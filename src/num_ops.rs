use std::ops::{Add, Div, Mul, Neg};

pub fn negate_opt_option<T>(v: &mut Option<T>)
where
    T: Neg<Output = T>,
{
    *v = v.take().map(|n| -n);
}

pub fn digit_remove_option<T>(v: &mut Option<T>)
where
    T: Copy + PartialEq + From<u8> + Div<Output = T>,
{
    *v = v.and_then(|n| {
        let ten = T::from(10u8);
        let zero = T::from(0u8);
        let divided = n / ten;

        if divided == zero {
            None
        } else {
            Some(divided)
        }
    })
}

pub fn digit_remove<T>(v: &mut T)
where
    T: Copy + PartialEq + From<u8> + Div<Output = T>,
{
    let ten = T::from(10u8);
    *v = *v / ten
}

pub fn digit_add_option<T>(v: &mut Option<T>, digit: u8)
where
    T: Copy + From<u8> + Add<Output = T> + Mul<Output = T>,
{
    assert!(digit < 10, "digit must be in 0..=9");

    let ten = T::from(10u8);
    let d = T::from(digit);

    *v = Some(match *v {
        Some(n) => n * ten + d,
        None => d,
    });
}

pub fn digit_add<T>(v: &mut T, digit: u8)
where
    T: Copy + From<u8> + Add<Output = T> + Mul<Output = T>,
{
    assert!(digit < 10, "digit must be in 0..=9");

    let ten = T::from(10u8);
    let d = T::from(digit);

    *v = *v * ten + d;
}
