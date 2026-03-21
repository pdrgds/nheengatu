pub fn calculate_price_cents(word_count: usize) -> u32 {
    match word_count {
        0..=30_000 => 300,
        30_001..=100_000 => 500,
        _ => 700 + (((word_count - 100_000) / 50_000) as u32 * 100),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn small() {
        assert_eq!(calculate_price_cents(10_000), 300);
    }
    #[test]
    fn medium() {
        assert_eq!(calculate_price_cents(50_000), 500);
    }
    #[test]
    fn large() {
        assert_eq!(calculate_price_cents(150_000), 800);
    }
}
