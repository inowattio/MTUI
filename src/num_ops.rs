use num_traits::{CheckedAdd, CheckedDiv, CheckedMul, CheckedNeg, CheckedSub, Zero};

pub fn negate_opt_option<T>(v: &mut Option<T>)
where
    T: CheckedNeg,
{
    *v = v.take().and_then(|n| n.checked_neg());
}

pub fn digit_remove_option<T>(v: &mut Option<T>)
where
    T: Copy + PartialEq + From<u8> + CheckedDiv<Output = T>,
{
    *v = v.map_or(None, |n| {
        let ten = T::from(10);
        let zero = T::from(0);

        if n.checked_div(&ten) == Some(zero) {
            return None;
        }

        let mut c = n;
        digit_remove(&mut c);
        Some(c)
    });
}

pub fn digit_remove<T>(v: &mut T)
where
    T: Copy + PartialEq + From<u8> + CheckedDiv<Output = T>,
{
    let ten = T::from(10u8);
    if let Some(c) = v.checked_div(&ten) {
        *v = c;
    }
}

pub fn digit_add_option<T>(v: &mut Option<T>, digit: u8)
where
    T: Copy + From<u8> + CheckedAdd<Output = T> + CheckedMul<Output = T>,
{
    *v = v.map_or(Some(T::from(digit)), |n| {
        let mut c = n;
        digit_add(&mut c, digit);
        Some(c)
    });
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

pub fn decrement_option_by<T>(v: &mut Option<T>, by: u16)
where
    T: Copy + From<u16> + CheckedSub<Output = T>,
{
    let zero = T::from(0);

    *v = v.map_or(Some(zero), |n| {
        let mut c = n;
        decrement_by(&mut c, by);
        Some(c)
    });
}

pub fn decrement_by<T>(v: &mut T, by: u16)
where
    T: Copy + From<u16> + CheckedSub<Output = T>,
{
    if let Some(c) = v.checked_sub(&by.into()) {
        *v = c;
    }
}

pub fn increment_option_by<T>(v: &mut Option<T>, by: u16)
where
    T: Copy + From<u16> + CheckedAdd<Output = T>,
{
    let zero = T::from(0);

    *v = v.map_or(Some(zero), |n| {
        let mut c = n;
        increment_by(&mut c, by);
        Some(c)
    });
}

pub fn increment_by<T>(v: &mut T, by: u16)
where
    T: Copy + From<u16> + CheckedAdd<Output = T>,
{
    if let Some(c) = v.checked_add(&by.into()) {
        *v = c;
    }
}

pub fn set_to_zero<T>(v: &mut T)
where
    T: Zero,
{
    *v = T::zero();
}

pub fn set_option_to_zero<T>(v: &mut Option<T>)
where
    T: Zero,
{
    if let Some(v) = v {
        *v = T::zero();
    }
}
