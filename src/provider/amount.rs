pub(super) fn amount_decimal(amount: &str, decimals: u8) -> Option<String> {
    if amount.is_empty() || !amount.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let trimmed = amount.trim_start_matches('0');
    let amount_str = if trimmed.is_empty() { "0" } else { trimmed };
    let decimals = decimals as usize;
    if decimals == 0 {
        return Some(amount_str.to_string());
    }
    if amount_str.len() <= decimals {
        let padding = decimals - amount_str.len();
        let result = format!("0.{:0>width$}{}", "", amount_str, width = padding);
        let result = result.trim_end_matches('0').to_string();
        let result = result.trim_end_matches('.').to_string();
        Some(result)
    } else {
        let split = amount_str.len() - decimals;
        let int_part = &amount_str[..split];
        let frac_part = &amount_str[split..];
        let frac_part = frac_part.trim_end_matches('0');
        if frac_part.is_empty() {
            Some(int_part.to_string())
        } else {
            Some(format!("{}.{}", int_part, frac_part))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_amount_decimal_exact() {
        assert_eq!(amount_decimal("1000000", 6).as_deref(), Some("1"));
        assert_eq!(amount_decimal("1234567", 6).as_deref(), Some("1.234567"));
        assert_eq!(amount_decimal("1200000", 6).as_deref(), Some("1.2"));
    }

    #[test]
    fn test_amount_decimal_with_leading_zeros() {
        assert_eq!(amount_decimal("000001000000", 6).as_deref(), Some("1"));
    }

    #[test]
    fn test_amount_decimal_small_values() {
        assert_eq!(
            amount_decimal("1", 18).as_deref(),
            Some("0.000000000000000001")
        );
        assert_eq!(
            amount_decimal("1000000000000000000", 18).as_deref(),
            Some("1")
        );
        assert_eq!(amount_decimal("42", 6).as_deref(), Some("0.000042"));
        assert_eq!(amount_decimal("0", 6).as_deref(), Some("0"));
    }

    #[test]
    fn test_amount_decimal_no_decimals() {
        assert_eq!(amount_decimal("500", 0).as_deref(), Some("500"));
    }

    #[test]
    fn test_amount_decimal_invalid() {
        assert_eq!(amount_decimal("", 6), None);
        assert_eq!(amount_decimal("abc", 6), None);
        assert_eq!(amount_decimal("12.34", 6), None);
    }
}
