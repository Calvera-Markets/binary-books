macro_rules! id_newtype {
    ($name:ident, $inner:ty) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name($inner);

        impl $name {
            pub const fn from_raw(raw: $inner) -> Self {
                Self(raw)
            }

            pub const fn raw(self) -> $inner {
                self.0
            }

            pub fn next(self) -> Self {
                Self(self.0.saturating_add(1))
            }
        }
    };
}

id_newtype!(OrderId, u64);
id_newtype!(MarketId, u64);
id_newtype!(OwnerId, u64);
id_newtype!(EventId, u64);
id_newtype!(Seq, u64);
id_newtype!(Ts, u64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_increments() {
        assert_eq!(OrderId::from_raw(7).next(), OrderId::from_raw(8));
        assert_eq!(Seq::from_raw(0).next().raw(), 1);
    }
}
