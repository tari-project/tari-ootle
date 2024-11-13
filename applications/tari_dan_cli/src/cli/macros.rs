#[macro_export]
macro_rules! loading {
    ( $text:expr, $call:expr ) => {
        {
            let mut loader = spinners::Spinner::new(spinners::Spinners::Dots, $text.into());
            let result = match $call {
                    Ok(res) => {
                        loader.stop_with_symbol("✅ ");
                        Ok(res)
                    }
                    Err(error) => {
                        loader.stop_with_symbol("❌ ");
                        Err(error)
                    }
            };
            result
        }
    };
}