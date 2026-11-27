//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod shared {
    use tari_template_lib::template_macro_deps::minicbor;

    #[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
    pub struct Discount {
        #[n(0)]
        pub bps: u64,
    }
}

#[tari_template_lib::prelude::template]
mod macro_scope_probe {
    use tari_template_lib::prelude::*;

    use super::shared::Discount;

    pub struct r#Match {
        total: u64,
    }

    pub struct Order {
        pub price: u64,
        pub qty: u64,
    }

    pub enum Side {
        Buy,
        Sell,
    }

    impl r#Match {
        pub fn new() -> Component<Self> {
            Component::new(Self { total: 0 })
                .with_access_rules(AccessRules::new().default(rule!(allow_all)))
                .create()
        }

        pub fn submit(
            &mut self,
            order: Order,
            side: Side,
            discount: Discount,
            _: u64,
            (bonus_a, bonus_b): (u64, u64),
        ) -> u64 {
            let gross = order.price * order.qty;
            let net = gross - discount.bps + bonus_a + bonus_b;
            match side {
                Side::Buy => self.total += net,
                Side::Sell => self.total = self.total.saturating_sub(net),
            }
            self.total
        }

        pub fn r#match(&self, r#type: u64) -> bool {
            self.total == r#type
        }
    }
}
