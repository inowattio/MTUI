use num_traits::{CheckedAdd, CheckedDiv, CheckedMul};

pub fn digit_remove<T>(v: &mut T)
where
    T: Copy + PartialEq + From<u8> + CheckedDiv<Output = T>,
{
    let ten = T::from(10u8);
    *v = *v / ten;
}

pub fn digit_add<T>(v: &mut T, digit: u8)
where
    T: Copy + From<u8> + CheckedAdd<Output = T> + CheckedMul<Output = T>,
{
    assert!(digit < 10, "digit must be in 0..=9");

    let ten = T::from(10u8);
    let d = T::from(digit);

    if let Some(c) = v.checked_mul(&ten).and_then(|n| n.checked_add(&d)) {
        *v = c;
    }
}

pub fn wrap_index(index: u16, len: u16, forward: bool) -> u16 {
    if len == 0 {
        return 0;
    }
    let last = len - 1;
    let index = index.min(last);
    match (forward, index) {
        (true, i) if i == last => 0,
        (true, i) => i + 1,
        (false, 0) => last,
        (false, i) => i - 1,
    }
}

pub fn cycle<T: Copy + PartialEq>(items: &[T], current: T, forward: bool) -> T {
    if items.is_empty() {
        return current;
    }
    let i = items.iter().position(|x| *x == current).unwrap_or(0);
    let n = items.len();
    let j = if forward {
        (i + 1) % n
    } else {
        (i + n - 1) % n
    };
    items[j]
}

pub fn step_hscroll(current: u16, max: u16, right: bool) -> u16 {
    const STEP: u16 = 8;
    let current = current.min(max);
    if right {
        (current + STEP).min(max)
    } else {
        current.saturating_sub(STEP)
    }
}

#[cfg(test)]
mod tests {
    use super::wrap_index;

    #[test]
    fn wrapping_moves_in_both_directions_and_round_trips_at_the_ends() {
        assert_eq!(wrap_index(0, 3, true), 1);
        assert_eq!(wrap_index(2, 3, true), 0);
        assert_eq!(wrap_index(0, 3, false), 2);
        assert_eq!(wrap_index(1, 3, false), 0);
    }

    #[test]
    fn empty_lists_and_stale_indices_are_safe() {
        assert_eq!(wrap_index(5, 0, true), 0);
        assert_eq!(wrap_index(5, 0, false), 0);
        assert_eq!(
            wrap_index(9, 3, true),
            0,
            "a stale index clamps to the end first"
        );
        assert_eq!(wrap_index(9, 3, false), 1);
        assert_eq!(wrap_index(u16::MAX, u16::MAX, true), 0);
        assert_eq!(wrap_index(0, u16::MAX, false), u16::MAX - 1);
    }
}
