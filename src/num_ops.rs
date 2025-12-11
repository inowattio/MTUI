use num_traits::{CheckedAdd, CheckedDiv, CheckedMul, CheckedNeg};

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
    *v = v.map(|n| {
        let mut c = n;
        digit_remove(&mut c);
        c
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
    *v = v.map(|n| {
        let mut c = n;
        digit_add(&mut c, digit);
        c
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
