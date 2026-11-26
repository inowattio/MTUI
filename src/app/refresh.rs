use super::{
    App, BackgroundTask, Change, ReadEntry, ReadError, RefreshTaskResult, RequestTrace, StatTarget,
    Trend,
};
use crate::compat::{self, Instant};
use crate::config::{AddressMode, BatchAnchor};
use crate::custom::CustomRule;
use crate::modbus::ModbusDevice;
use crate::register::{RegisterCell, RegisterCellValue, RegisterType};
use crate::state::{ConnectionStatus, ReadPanel, State};
use chrono::{Local, Utc};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

impl App {
    async fn read_words(
        device: &ModbusDevice,
        register_type: RegisterType,
        position: u16,
        amount: u16,
        trace: &mut RequestTrace,
    ) -> Result<(Vec<u16>, Duration), anyhow::Error> {
        trace.attempt = Some(StatTarget::Read(register_type, position, amount));
        device
            .read_timed(None, register_type, position, amount)
            .await
    }

    async fn acquire_data_with(
        device: &ModbusDevice,
        amount: u16,
        position: u16,
        register_type: RegisterType,
        trace: &mut RequestTrace,
    ) -> Result<Vec<RegisterCellValue>, anyhow::Error> {
        let (values, latency) =
            Self::read_words(device, register_type, position, amount, trace).await?;
        trace.latencies.push(latency);

        Ok(values
            .into_iter()
            .enumerate()
            .map(|(i, v)| ((register_type, position + i as u16), v))
            .collect())
    }

    async fn acquire_cells_with(
        device: &ModbusDevice,
        regs: &[RegisterCell],
        batch: u16,
        full_spans: &BTreeMap<RegisterCell, u16>,
        address_mode: AddressMode,
        trace: &mut RequestTrace,
    ) -> Result<Vec<RegisterCellValue>, anyhow::Error> {
        let batch = batch.max(1) as usize;
        let mut collection = Vec::with_capacity(regs.len());

        let mut i = 0usize;
        while i < regs.len() {
            let (kind, start_addr) = regs[i];
            let run_len = read_run_len(regs, i, batch, full_spans);

            let (values, latency) =
                Self::read_words(device, kind, start_addr, run_len as u16, trace).await?;
            anyhow::ensure!(
                values.len() == run_len,
                "Expected {run_len} value(s) at {}, got {}",
                address_mode.display(start_addr),
                values.len()
            );
            trace.latencies.push(latency);

            collection.extend(regs[i..i + run_len].iter().copied().zip(values));

            i += run_len;
        }

        Ok(collection)
    }

    pub fn read_window(&self) -> (u16, u16) {
        let sweeping = self.sweep.active;
        let position = self.read().position;
        let amount = if sweeping && self.sweep.errored {
            1
        } else if sweeping {
            let remaining = self.sweep.to.saturating_sub(position).saturating_add(1);
            self.config.batch.size.clamp(1, remaining)
        } else {
            self.config.batch.size.max(1)
        };
        let max_read_start = u16::MAX - (amount - 1);
        let start = if sweeping {
            position
        } else {
            match self.config.batch.anchor {
                BatchAnchor::Start => position,
                BatchAnchor::Middle => position.saturating_sub(amount / 2),
                BatchAnchor::End => position.saturating_sub(amount - 1),
            }
        }
        .min(max_read_start);
        let main = matches!(self.read().panel, ReadPanel::Main | ReadPanel::Matrix);
        if sweeping || !main || !self.config.batch.read_full_customs {
            return (start, amount);
        }
        let register_type = self.read().register_type;
        let runs = custom_runs(&self.custom_rules, register_type);
        let (start, end) = cover_runs(start, start + (amount - 1), &runs);
        (start, (end - start).saturating_add(1))
    }

    pub fn main_extra_cells(&self) -> Vec<RegisterCell> {
        if self.sweep.active || !self.config.batch.read_full_customs {
            return Vec::new();
        }
        let (start, amount) = self.read_window();
        let end = start.saturating_add(amount - 1);
        custom_words_outside(&self.custom_rules, self.read().register_type, start..=end)
    }

    pub fn refresh(&mut self) {
        if self.background_task.is_some() || !self.is_reading() {
            return;
        }
        let Some(device) = self.device.clone() else {
            return;
        };

        let sweeping = self.sweep.active;
        let (read_start, amount) = self.read_window();
        self.scroll_to_cursor();
        let (panel, register_type) = {
            let p = self.read_mut();
            p.loading = true;
            p.read_started = Instant::now();
            (p.panel, p.register_type)
        };

        let read_main = sweeping || matches!(panel, ReadPanel::Main | ReadPanel::Matrix);

        let mut graph_registers = self.graph_extra_registers();
        if read_main {
            let read_end = read_start.saturating_add(amount - 1);
            graph_registers.extend(self.main_extra_cells());
            graph_registers.retain(|&(kind, address)| {
                kind != register_type || !(read_start..=read_end).contains(&address)
            });
            graph_registers.sort_unstable();
            graph_registers.dedup();
        }

        let panel_registers = if read_main {
            graph_registers
        } else {
            let mut regs = self.panel_refresh_window(amount as usize);
            regs.extend(graph_registers);
            regs.sort();
            regs.dedup();
            regs
        };

        if !read_main && panel_registers.is_empty() {
            self.read_mut().finish_read();
            return;
        }
        self.connection = ConnectionStatus::Reading;

        let full_spans = if self.config.batch.read_full_customs {
            custom_full_spans(&self.custom_rules)
        } else {
            BTreeMap::new()
        };

        let address_mode = self.interpreter.address_mode();
        self.background_task = Some(BackgroundTask::Refresh(compat::spawn(async move {
            let read_began = Instant::now();
            let mut values = Vec::new();
            let mut trace = RequestTrace::default();
            let mut error = None;
            if read_main {
                match Self::acquire_data_with(
                    &device,
                    amount,
                    read_start,
                    register_type,
                    &mut trace,
                )
                .await
                {
                    Ok(data) => values = data,
                    Err(e) => error = Some(ReadError::from(e)),
                }
            }
            if error.is_none() && !panel_registers.is_empty() {
                match Self::acquire_cells_with(
                    &device,
                    &panel_registers,
                    amount,
                    &full_spans,
                    address_mode,
                    &mut trace,
                )
                .await
                {
                    Ok(data) => values.extend(data),
                    Err(e) => error = Some(ReadError::from(e)),
                }
            }
            let read_duration = read_began.elapsed();

            RefreshTaskResult {
                register_type,
                main_window: read_main,
                values,
                trace,
                error,
                read_duration,
            }
        })));
    }

    pub(super) fn apply_refresh_result(&mut self, result: RefreshTaskResult) {
        for &latency in &result.trace.latencies {
            self.stats().reads.record_ok(latency);
        }
        if let Some(e) = &result.error {
            self.stats()
                .reads
                .record_error(&e.message, result.trace.attempt);
        }
        if !self.is_reading() {
            self.read_or_previous_mut().finish_read();
            self.settle_connection(result.error.as_ref());
            return;
        }
        if result.main_window
            && !matches!(
                &self.state,
                State::Read(params) if params.register_type == result.register_type
            )
        {
            self.read_mut().finish_read();
            self.settle_connection(result.error.as_ref());
            return;
        }

        self.store_values(&result.values);
        self.sync_auto_widths();

        {
            let params = self.read_mut();
            params.read_duration = Some(result.read_duration);
            params.finish_read();
            params.read_error = result.error.as_ref().map(|e| e.message.clone());
        }
        self.settle_connection(result.error.as_ref());

        if self.sweep.active && result.main_window {
            self.advance_sweep(result.error.is_some());
        }
    }

    pub(super) fn store_values(&mut self, values: &[(RegisterCell, u16)]) {
        let read_at = Utc::now();
        let time_text: Arc<str> = read_at
            .with_timezone(&Local)
            .format("%H:%M:%S.%3f")
            .to_string()
            .into();
        let history_cap = (self.config.graph.history as usize).max(1);
        let display = &self.config.display;
        let owners = if display.changed_trend && display.changed_trend_custom {
            self.custom_owners_reading(values)
        } else {
            Vec::new()
        };
        let levels: Vec<_> = owners
            .into_iter()
            .map(|owner| (owner, self.custom_level(owner)))
            .collect();

        for &(cell, value) in values {
            let entry = ReadEntry {
                value,
                at: read_at,
                time_text: Arc::clone(&time_text),
            };
            let previous = self.read_log.insert(cell, entry);
            let order = self.config.device.word_order.for_kind(cell.0);
            if let Some(prev) = previous
                && let Some(trend) = Trend::between(order.single(prev.value), order.single(value))
            {
                let flipped = prev.value ^ value;
                let change = Change {
                    at: read_at,
                    trend,
                    flipped,
                };
                self.changed.insert(cell, change);
            }

            let history = self.value_history.entry(cell).or_default();
            history.push_back((value, read_at));
            history.retain_back(history_cap);
        }

        for (owner, before) in levels {
            let after = self.custom_level(owner);
            let trend = before.zip(after).and_then(|(b, a)| Trend::between(b, a));
            if let Some(trend) = trend {
                let change = Change {
                    at: read_at,
                    trend,
                    flipped: 0,
                };
                self.custom_changed.insert(owner, change);
            }
        }
    }
}

fn custom_runs(rules: &BTreeMap<RegisterCell, CustomRule>, kind: RegisterType) -> Vec<(u16, u16)> {
    let mut runs = Vec::new();
    for (&(rule_kind, _), rule) in rules {
        if rule_kind != kind {
            continue;
        }
        let words: Vec<u16> = rule.word_addresses().collect();
        let mut run_start = 0usize;
        for i in 1..=words.len() {
            if i < words.len() && words[i - 1].checked_add(1) == Some(words[i]) {
                continue;
            }
            if i - run_start > 1 {
                runs.push((words[run_start], words[i - 1]));
            }
            run_start = i;
        }
    }
    runs
}

fn custom_words_outside(
    rules: &BTreeMap<RegisterCell, CustomRule>,
    kind: RegisterType,
    window: std::ops::RangeInclusive<u16>,
) -> Vec<RegisterCell> {
    rules
        .range((kind, 0)..=(kind, u16::MAX))
        .filter(|(_, rule)| rule.word_addresses().any(|a| window.contains(&a)))
        .flat_map(|(_, rule)| rule.source_addresses())
        .filter(|a| !window.contains(a))
        .map(|a| (kind, a))
        .collect()
}

fn cover_runs(mut start: u16, mut end: u16, runs: &[(u16, u16)]) -> (u16, u16) {
    loop {
        let before = (start, end);
        for &(run_start, run_end) in runs {
            if run_start <= end && run_end >= start {
                start = start.min(run_start);
                end = end.max(run_end);
            }
        }
        if (start, end) == before {
            return before;
        }
    }
}

fn custom_full_spans(rules: &BTreeMap<RegisterCell, CustomRule>) -> BTreeMap<RegisterCell, u16> {
    let mut spans = BTreeMap::new();
    for (&(kind, _), rule) in rules {
        let words: Vec<u16> = rule.word_addresses().collect();
        let mut run_start = 0usize;
        for i in 1..=words.len() {
            if i < words.len() && words[i - 1].checked_add(1) == Some(words[i]) {
                continue;
            }
            let end = words[i - 1];
            if i - run_start > 1 {
                for &word in &words[run_start..i] {
                    let span = spans.entry((kind, word)).or_insert(end);
                    *span = (*span).max(end);
                }
            }
            run_start = i;
        }
    }
    spans
}

fn read_run_len(
    regs: &[RegisterCell],
    start: usize,
    batch: usize,
    full_spans: &BTreeMap<RegisterCell, u16>,
) -> usize {
    let (kind, start_addr) = regs[start];
    let mut run_len = 1usize;
    while start + run_len < regs.len() {
        let Some(next_addr) = start_addr.checked_add(run_len as u16) else {
            break;
        };
        let last_addr = next_addr - 1;
        let in_span = full_spans
            .get(&(kind, last_addr))
            .is_some_and(|&end| end > last_addr);
        if run_len >= batch && !in_span {
            break;
        }
        if regs[start + run_len] != (kind, next_addr) {
            break;
        }
        run_len += 1;
    }
    run_len
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_arch = "wasm32"))]
    use super::{App, StatTarget};
    use super::{cover_runs, custom_full_spans, custom_runs, custom_words_outside, read_run_len};
    #[cfg(not(target_arch = "wasm32"))]
    use crate::config::Config;
    use crate::custom::{CustomRepr, CustomRule};
    use crate::register::{RegisterCell, RegisterType};
    #[cfg(not(target_arch = "wasm32"))]
    use crate::state::ReadPanel;
    #[cfg(not(target_arch = "wasm32"))]
    use std::assert_matches;
    use std::collections::BTreeMap;

    const H: RegisterType = RegisterType::Holding;

    fn rules(entries: &[(u16, CustomRepr, &[u16])]) -> BTreeMap<RegisterCell, CustomRule> {
        entries
            .iter()
            .map(|&(address, repr, next)| {
                let rule = CustomRule {
                    address,
                    repr,
                    next: next.to_vec(),
                    ..Default::default()
                };
                ((H, address), rule)
            })
            .collect()
    }

    fn cells(addrs: &[u16]) -> Vec<RegisterCell> {
        addrs.iter().map(|&a| (H, a)).collect()
    }

    #[test]
    fn spans_cover_contiguous_rules() {
        let spans = custom_full_spans(&rules(&[(100, CustomRepr::F64, &[])]));
        for addr in 100..=103 {
            assert_eq!(spans.get(&(H, addr)), Some(&103));
        }
        assert_eq!(spans.get(&(H, 104)), None);
    }

    #[test]
    fn spans_skip_single_words_and_split_on_jumps() {
        let spans = custom_full_spans(&rules(&[
            (7, CustomRepr::U16, &[]),
            (520, CustomRepr::F64, &[524]),
        ]));
        assert_eq!(spans.get(&(H, 7)), None);
        assert_eq!(spans.get(&(H, 520)), None);
        for addr in 524..=526 {
            assert_eq!(spans.get(&(H, addr)), Some(&526));
        }
    }

    #[test]
    fn runs_list_contiguous_words_of_the_requested_kind() {
        let mut rules = rules(&[
            (7, CustomRepr::U16, &[]),
            (100, CustomRepr::F64, &[]),
            (520, CustomRepr::F64, &[524]),
        ]);
        rules.insert(
            (RegisterType::Input, 3),
            CustomRule {
                address: 3,
                repr: CustomRepr::U32,
                ..Default::default()
            },
        );
        assert_eq!(custom_runs(&rules, H), vec![(100, 103), (524, 526)]);
        assert_eq!(custom_runs(&rules, RegisterType::Input), vec![(3, 4)]);
    }

    #[test]
    fn jumped_words_of_touched_rules_are_read_separately() {
        let rules = rules(&[
            (17, CustomRepr::U32, &[30]),
            (40, CustomRepr::U64, &[41, 60, 61]),
            (80, CustomRepr::U32, &[]),
        ]);
        assert_eq!(custom_words_outside(&rules, H, 10..=20), cells(&[30]));
        assert_eq!(custom_words_outside(&rules, H, 30..=30), cells(&[17]));
        assert_eq!(custom_words_outside(&rules, H, 40..=42), cells(&[60, 61]));
        assert_eq!(custom_words_outside(&rules, H, 70..=90), Vec::new());
        assert_eq!(
            custom_words_outside(&rules, RegisterType::Input, 10..=20),
            Vec::new()
        );
    }

    #[test]
    fn a_scale_register_is_read_with_its_rule_but_does_not_pull_it_in() {
        use crate::custom::{CustomScale, ScaleMode, ScaleRepr};
        let mut rules = rules(&[(17, CustomRepr::U32, &[])]);
        rules.get_mut(&(H, 17)).unwrap().scale = Some(CustomScale {
            register: 50,
            mode: ScaleMode::Pow10,
            repr: ScaleRepr::Sunspec,
            ..Default::default()
        });
        assert_eq!(custom_words_outside(&rules, H, 10..=20), cells(&[50]));
        assert_eq!(custom_words_outside(&rules, H, 15..=17), cells(&[18, 50]));
        assert_eq!(custom_words_outside(&rules, H, 45..=55), Vec::new());
        assert_eq!(custom_runs(&rules, H), vec![(17, 18)]);

        let scale = rules.get_mut(&(H, 17)).unwrap().scale.as_mut().unwrap();
        scale.mode = ScaleMode::Div;
        scale.repr = ScaleRepr::Plain(CustomRepr::F32);
        assert_eq!(custom_words_outside(&rules, H, 10..=20), cells(&[50, 51]));
    }

    #[test]
    fn covering_grows_the_window_over_touched_runs_only() {
        let runs = [(100, 103), (104, 105), (200, 201)];
        assert_eq!(cover_runs(101, 101, &runs), (100, 103));
        assert_eq!(cover_runs(103, 103, &runs), (100, 103));
        assert_eq!(cover_runs(103, 104, &runs), (100, 105));
        assert_eq!(cover_runs(50, 60, &runs), (50, 60));
        assert_eq!(cover_runs(99, 200, &runs), (99, 201));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn the_main_read_covers_the_custom_under_the_cursor_when_enabled() {
        let mut app = App::boot(Config::default(), String::new()).await;
        app.config.batch.size = 1;
        app.config.batch.anchor = crate::config::BatchAnchor::Start;
        app.read_mut().register_type = H;
        app.custom_rules =
            rules(&[(100, CustomRepr::F32, &[]), (520, CustomRepr::F64, &[524])]).into();

        app.read_mut().position = 101;
        assert_eq!(app.read_window(), (101, 1), "off by default");

        app.config.batch.read_full_customs = true;
        assert_eq!(
            app.read_window(),
            (100, 2),
            "second half pulls in the first"
        );
        app.read_mut().position = 100;
        assert_eq!(app.read_window(), (100, 2));
        app.read_mut().position = 102;
        assert_eq!(app.read_window(), (102, 1), "neighbours are left alone");
        app.read_mut().position = 520;
        assert_eq!(app.read_window(), (520, 1), "a jump is not bridged");
        app.read_mut().position = 525;
        assert_eq!(
            app.read_window(),
            (524, 3),
            "the contiguous tail is covered"
        );

        app.read_mut().panel = ReadPanel::Pinned;
        assert_eq!(
            app.read_window(),
            (525, 1),
            "list panels keep the plain batch"
        );
        app.read_mut().panel = ReadPanel::Main;

        app.read_mut().register_type = RegisterType::Input;
        app.read_mut().position = 101;
        assert_eq!(
            app.read_window(),
            (101, 1),
            "other register types are unaffected"
        );
    }

    #[test]
    fn run_respects_batch_and_gaps() {
        let none = BTreeMap::new();
        assert_eq!(read_run_len(&cells(&[10, 11, 12, 13, 14]), 0, 2, &none), 2);
        assert_eq!(read_run_len(&cells(&[10, 11, 20]), 0, 5, &none), 2);
        assert_eq!(
            read_run_len(&[(H, 10), (RegisterType::Input, 11)], 0, 5, &none),
            1
        );
    }

    #[test]
    fn run_extends_to_cover_custom_span() {
        let spans = custom_full_spans(&rules(&[(100, CustomRepr::F64, &[])]));
        assert_eq!(read_run_len(&cells(&[100, 101, 102, 103]), 0, 2, &spans), 4);
        assert_eq!(
            read_run_len(&cells(&[99, 100, 101, 102, 103]), 0, 2, &spans),
            5
        );
        assert_eq!(
            read_run_len(&cells(&[100, 101, 102, 103, 104]), 0, 2, &spans),
            4
        );
    }

    #[test]
    fn run_cannot_extend_past_missing_cells() {
        let spans = custom_full_spans(&rules(&[(100, CustomRepr::F64, &[])]));
        assert_eq!(read_run_len(&cells(&[100, 101, 105]), 0, 2, &spans), 2);
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn settle(app: &mut App) {
        crate::app::settle_until(app, |app| app.background_task.is_none(), "background task").await;
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn trends_compare_the_byte_ordered_value() {
        use crate::app::Trend;
        use crate::modbus::WordOrder;

        let mut app = App::boot(Config::default(), String::new()).await;
        app.config.device.word_order = WordOrder::BADC;
        let holding = (RegisterType::Holding, 900);
        let coil = (RegisterType::Coil, 900);
        app.store_values(&[(holding, 0xFF00), (coil, 0)]);
        app.store_values(&[(holding, 0x0001), (coil, 1)]);
        assert_eq!(app.changed[&holding].trend, Trend::Up);
        assert_eq!(app.changed[&coil].trend, Trend::Up);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn every_modbus_request_of_a_refresh_counts_as_a_read() {
        let mut app = App::boot(Config::default(), String::new()).await;
        {
            let p = app.read_mut();
            p.register_type = RegisterType::Input;
            p.position = 0;
            p.graph = true;
            p.graph_series = vec![(RegisterType::Input, 200), (RegisterType::Input, 400)];
        }

        app.refresh();
        settle(&mut app).await;

        assert_eq!(app.stats().reads.ok, 3, "main window plus two graph runs");
        assert_eq!(app.stats().reads.errors, 0);
        assert!(app.stats().reads.latency().is_some());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn a_failing_request_counts_once_after_the_ones_before_it() {
        let mut app = App::boot(Config::default(), String::new()).await;
        {
            let p = app.read_mut();
            p.register_type = RegisterType::Input;
            p.position = 0;
            p.graph = true;
            p.graph_series = vec![(RegisterType::Input, 64), (RegisterType::Input, 400)];
        }

        app.refresh();
        settle(&mut app).await;

        assert_eq!(app.stats().reads.ok, 1, "only the main window answered");
        assert_eq!(
            app.stats().reads.errors,
            1,
            "the run after the failure is never sent"
        );
        let target = app.stats().reads.last_error().and_then(|e| e.target);
        assert_matches!(
            target,
            Some(StatTarget::Read(RegisterType::Input, 64, _)),
            "the error points at the failing request, got {target:?}"
        );
    }
}
