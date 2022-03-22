pub fn format_percentage_without_unit(value: f64) -> String {
    let percentage = value * 100.0;
    if (percentage - percentage.round()).abs() < 0.001 {
        // No fraction. Omit zeros after dot.
        format!("{:.4}", percentage)
    } else {
        // Has fraction. We want to display these.
        format!("{:.4}", percentage)
    }
}

pub fn parse_percentage_without_unit(text: &str) -> Result<f64, &'static str> {
    let percentage: f64 = text.parse().map_err(|_| "not a valid decimal value")?;
    Ok(percentage / 100.0)
}
