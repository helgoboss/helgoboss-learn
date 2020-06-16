//
// enum LazyValState<F, T>
//     where
//         F: FnOnce() -> T,
// {
//     Empty(F),
//     Filled(T),
// }

use lazycell::LazyCell;
use std::cell::RefCell;

pub trait Lazy<T> {
    fn get(&self) -> &T;
}

pub struct LazyVal<T, F>
where
    F: FnOnce() -> T,
{
    cached: LazyCell<T>,
    evaluate: RefCell<Option<F>>,
}

impl<T, F> LazyVal<T, F>
where
    F: FnOnce() -> T,
{
    pub fn new(evaluate: F) -> LazyVal<T, F> {
        LazyVal {
            cached: LazyCell::new(),
            evaluate: RefCell::new(Some(evaluate)),
        }
    }
}

impl<T, F> Lazy<T> for LazyVal<T, F>
where
    F: FnOnce() -> T,
{
    fn get(&self) -> &T {
        if let Some(cached) = self.cached.borrow() {
            cached
        } else {
            let f = self.evaluate.replace(None).expect("impossible");
            self.cached.fill(f());
            self.cached.borrow().expect("impossible")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basics() {
        let lazy_val = LazyVal::new(|| factorial(20));
        assert_eq!(std::mem::size_of_val(&lazy_val), 40);
        assert_eq!(lazy_val.get(), &2432902008176640000);
        assert_eq!(lazy_val.get(), &2432902008176640000);
    }

    fn factorial(n: u128) -> u128 {
        if n < 2 { 1 } else { n * factorial(n - 1) }
    }
}
