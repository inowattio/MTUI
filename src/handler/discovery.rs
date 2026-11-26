use crate::app::App;
use crate::input::{KeyCode, KeyEvent};
use crate::modbus::{DataBits, Parity, StopBits, WordOrder};
use crate::num_ops::{cycle, digit_add, digit_remove};
use crate::state::{DiscoveryField, DiscoveryParams, InterfaceKind};

pub(super) fn handle_discovery_key(key_event: KeyEvent, app: &mut App) {
    let kb = app.config.keybinds;
    let Some(field) = app.discovery().map(DiscoveryParams::current_field) else {
        return;
    };

    match key_event.code {
        KeyCode::Esc => app.close_popup(),
        KeyCode::Enter => match field {
            DiscoveryField::ScanNetwork => app.start_network_scan(),
            DiscoveryField::ScanMethod => {
                if let Some(d) = app.discovery_mut() {
                    d.cycle_scan_method(true);
                    d.status = None;
                }
            }
            DiscoveryField::ScanRepr => {
                if let Some(d) = app.discovery_mut() {
                    d.scan_ascii.toggle();
                }
            }
            DiscoveryField::Port(index) => app.choose_port(index),
            DiscoveryField::Found(index) => app.use_found_ip(index),
            _ => app.discovery_connect(),
        },
        KeyCode::Up | KeyCode::Down => {
            if let Some(d) = app.discovery_mut() {
                d.move_cursor(key_event.code == KeyCode::Down);
            }
        }
        c if c == kb.panel => {
            if let Some(d) = app.discovery_mut() {
                d.toggle_column();
            }
        }
        KeyCode::Left | KeyCode::Right => {
            let show_mock_device = app.config.display.mock_device;
            if let Some(d) = app.discovery_mut() {
                cycle_field(d, field, key_event.code == KeyCode::Right, show_mock_device);
                d.status = None;
            }
        }
        KeyCode::Backspace => {
            if let Some(d) = app.discovery_mut() {
                match field {
                    DiscoveryField::Ip => {
                        d.ip.pop();
                    }
                    DiscoveryField::CustomPath => {
                        d.custom_path.pop();
                    }
                    DiscoveryField::Baud => digit_remove(&mut d.baud_rate),
                    DiscoveryField::NetPort => digit_remove(&mut d.net_port),
                    DiscoveryField::UnitId => digit_remove(&mut d.unit_id),
                    DiscoveryField::ConnectTimeout => digit_remove(&mut d.connect_timeout_ms),
                    DiscoveryField::CommandTimeout => digit_remove(&mut d.command_timeout_ms),
                    DiscoveryField::BetweenCommands => digit_remove(&mut d.between_commands_ms),
                    _ => {}
                }
                d.status = None;
            }
        }
        KeyCode::Char(c) => {
            if let Some(d) = app.discovery_mut() {
                let digit = (c as u8).saturating_sub(b'0');
                match field {
                    DiscoveryField::Ip if c.is_ascii_digit() || c == '.' => d.ip.push(c),
                    DiscoveryField::CustomPath if !c.is_control() => d.custom_path.push(c),
                    DiscoveryField::Baud if c.is_ascii_digit() => {
                        digit_add(&mut d.baud_rate, digit);
                    }
                    DiscoveryField::NetPort if c.is_ascii_digit() => {
                        digit_add(&mut d.net_port, digit);
                    }
                    DiscoveryField::UnitId if c.is_ascii_digit() => {
                        digit_add(&mut d.unit_id, digit);
                    }
                    DiscoveryField::ConnectTimeout if c.is_ascii_digit() => {
                        digit_add(&mut d.connect_timeout_ms, digit);
                    }
                    DiscoveryField::CommandTimeout if c.is_ascii_digit() => {
                        digit_add(&mut d.command_timeout_ms, digit);
                    }
                    DiscoveryField::BetweenCommands if c.is_ascii_digit() => {
                        digit_add(&mut d.between_commands_ms, digit);
                    }
                    _ => {}
                }
                d.status = None;
            }
        }
        _ => {}
    }
}

fn cycle_field(
    d: &mut DiscoveryParams,
    field: DiscoveryField,
    forward: bool,
    show_mock_device: bool,
) {
    let kinds: Vec<InterfaceKind> = InterfaceKind::ALL
        .into_iter()
        .filter(|&kind| show_mock_device || kind != InterfaceKind::Mock)
        .collect();

    match field {
        DiscoveryField::Interface => d.set_interface(cycle(&kinds, d.interface, forward)),
        DiscoveryField::Baud => d.cycle_baud(forward),
        DiscoveryField::ScanMethod => d.cycle_scan_method(forward),
        DiscoveryField::ScanRepr => d.scan_ascii.toggle(),
        DiscoveryField::ScanPrefix => d.step_scan_prefix(forward),
        DiscoveryField::DataBits => d.data_bits = cycle(&DataBits::ALL, d.data_bits, forward),
        DiscoveryField::Parity => d.parity = cycle(&Parity::ALL, d.parity, forward),
        DiscoveryField::StopBits => d.stop_bits = cycle(&StopBits::ALL, d.stop_bits, forward),
        DiscoveryField::WordOrder => d.word_order = cycle(&WordOrder::ALL, d.word_order, forward),
        _ => {}
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use crate::app::App;
    use crate::handler::tests::app;
    use crate::handler::{handle_key_events, handle_paste};
    use crate::input::{KeyCode, KeyEvent};
    use crate::state::{DiscoveryField, DiscoveryParams, InterfaceKind, PopupKind};

    #[tokio::test]
    async fn the_custom_serial_path_is_typed_and_pasted() {
        let mut app = app().await;
        app.open_discovery();
        {
            let d = app.discovery_mut().unwrap();
            *d = DiscoveryParams {
                ports: vec!["/dev/ttyUSB0".to_string()],
                ..DiscoveryParams::default()
            };
            d.set_interface(InterfaceKind::Serial);
            d.toggle_column();
            d.move_cursor(true); // past the single port row onto the custom path
        }
        assert_eq!(
            app.discovery().unwrap().current_field(),
            DiscoveryField::CustomPath
        );

        for c in "/dev/ttyAMA0".chars() {
            handle_key_events(KeyEvent::new(KeyCode::Char(c)), &mut app);
        }
        handle_key_events(KeyEvent::new(KeyCode::Backspace), &mut app);
        handle_paste("1\n".to_string(), &mut app);

        let d = app.discovery().unwrap();
        assert_eq!(d.custom_path, "/dev/ttyAMA1");
        assert_eq!(d.serial_path().as_deref(), Some("/dev/ttyAMA1"));
        assert_eq!(app.popup_kind(), Some(PopupKind::Discovery), "still open");
    }

    #[tokio::test]
    async fn the_baud_rate_is_typed_or_stepped() {
        let mut app = app().await;
        app.open_discovery();
        {
            let d = app.discovery_mut().unwrap();

            *d = DiscoveryParams::default();
            d.set_interface(InterfaceKind::Serial);
            d.toggle_column();
            d.move_cursor(true);
        }
        assert_eq!(
            app.discovery().unwrap().current_field(),
            DiscoveryField::Baud
        );

        for _ in 0..4 {
            handle_key_events(KeyEvent::new(KeyCode::Backspace), &mut app);
        }
        assert_eq!(app.discovery().unwrap().baud_rate, 0, "9600 erased");
        for c in "250000".chars() {
            handle_key_events(KeyEvent::new(KeyCode::Char(c)), &mut app);
        }
        assert_eq!(app.discovery().unwrap().baud_rate, 250_000);

        handle_key_events(KeyEvent::new(KeyCode::Right), &mut app);
        assert_eq!(
            app.discovery().unwrap().baud_rate,
            460_800,
            "next preset above"
        );
        handle_key_events(KeyEvent::new(KeyCode::Left), &mut app);
        handle_key_events(KeyEvent::new(KeyCode::Left), &mut app);
        assert_eq!(app.discovery().unwrap().baud_rate, 115_200);
    }

    #[tokio::test]
    async fn the_interface_cycle_includes_rtu_over_tcp() {
        let mut app = app().await;
        app.open_discovery();
        let kind = |app: &App| app.discovery().unwrap().interface;
        assert_eq!(kind(&app), InterfaceKind::Mock);
        for expected in [
            InterfaceKind::Serial,
            InterfaceKind::Tcp,
            InterfaceKind::RtuOverTcp,
            InterfaceKind::Mock,
        ] {
            handle_key_events(KeyEvent::new(KeyCode::Right), &mut app);
            assert_eq!(kind(&app), expected);
        }

        app.config.display.mock_device = false;
        app.discovery_mut()
            .unwrap()
            .set_interface(InterfaceKind::Serial);
        for expected in [
            InterfaceKind::Tcp,
            InterfaceKind::RtuOverTcp,
            InterfaceKind::Serial,
        ] {
            handle_key_events(KeyEvent::new(KeyCode::Right), &mut app);
            assert_eq!(kind(&app), expected);
        }
    }
}
