use num_traits::{CheckedAdd, CheckedDiv, CheckedMul};

pub fn digit_remove<T>(v: &mut T)
where
    T: Copy + PartialEq + From<u8> + CheckedDiv<Output = T>,
{
    let ten = T::from(10u8);
    if let Some(c) = v.checked_div(&ten) {
        *v = c;
    }
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
    if forward {
        (index + 1) % len
    } else {
        (index + len - 1) % len
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
