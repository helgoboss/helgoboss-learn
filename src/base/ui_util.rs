pub fn format_percentage_without_unit(value: f64) -> String {
    let percentage = value * 100.0;
    format!("{:.4}", percentage)
}

pub fn parse_percentage_without_unit(text: &str) -> Result<f64, &'static str> {
    let percentage: f64 = text.parse().map_err(|_| "not a valid decimal value")?;
    Ok(percentage / 100.0)
}
