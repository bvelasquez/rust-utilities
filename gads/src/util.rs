/// Strip `customers/XXX/` prefix and dashes from customer IDs.
pub fn normalize_customer_id(id: &str) -> String {
    id.replace("customers/", "")
        .replace('-', "")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_customer_id() {
        assert_eq!(normalize_customer_id("123-456-7890"), "1234567890");
        assert_eq!(normalize_customer_id("customers/1234567890"), "1234567890");
    }
}
