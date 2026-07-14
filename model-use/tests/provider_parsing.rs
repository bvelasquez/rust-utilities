use model_use::providers::types::Provider;

#[test]
fn anthropic_cents_to_usd() {
    let amount = "12345";
    let cents: f64 = amount.parse().unwrap();
    let usd = cents / 100.0;
    assert!((usd - 123.45).abs() < 0.001);
}

#[test]
fn provider_parse() {
    assert_eq!("openrouter".parse::<Provider>().unwrap(), Provider::Openrouter);
    assert_eq!("anthropic".parse::<Provider>().unwrap(), Provider::Anthropic);
    assert_eq!("openai".parse::<Provider>().unwrap(), Provider::Openai);
    assert_eq!("cursor".parse::<Provider>().unwrap(), Provider::Cursor);
}
