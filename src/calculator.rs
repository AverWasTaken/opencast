//! Offline arithmetic and dimension-checked unit conversions. Never executes code.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub value: f64,
    pub unit: String,
}
impl Answer {
    pub fn display(&self) -> String {
        let value = if self.value == 0.0 { 0.0 } else { self.value };
        let number = if value != 0.0 && !(1e-8..1e12).contains(&value.abs()) {
            format!("{value:.8e}")
        } else {
            format!("{value:.8}")
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_owned()
        };
        if self.unit.is_empty() {
            number
        } else {
            format!("{number} {}", self.unit)
        }
    }
}
// Base units: metre, kilogram, second, litre, byte, kelvin, radian.
fn unit(name: &str) -> Option<(&'static str, f64, f64)> {
    Some(match name.to_lowercase().as_str() {
        "mm" => ("length", 0.001, 0.0),
        "cm" => ("length", 0.01, 0.0),
        "m" | "meter" | "meters" | "metre" | "metres" => ("length", 1.0, 0.0),
        "km" => ("length", 1000.0, 0.0),
        "in" | "inch" | "inches" => ("length", 0.0254, 0.0),
        "ft" | "foot" | "feet" => ("length", 0.3048, 0.0),
        "yd" | "yards" => ("length", 0.9144, 0.0),
        "mi" | "mile" | "miles" => ("length", 1609.344, 0.0),
        "mg" => ("mass", 0.000001, 0.0),
        "g" => ("mass", 0.001, 0.0),
        "kg" => ("mass", 1.0, 0.0),
        "lb" | "lbs" => ("mass", 0.45359237, 0.0),
        "oz" => ("mass", 0.028349523125, 0.0),
        "ms" => ("time", 0.001, 0.0),
        "s" | "sec" | "seconds" => ("time", 1.0, 0.0),
        "min" | "minutes" => ("time", 60.0, 0.0),
        "h" | "hr" | "hours" => ("time", 3600.0, 0.0),
        "d" | "days" => ("time", 86400.0, 0.0),
        "ml" => ("volume", 0.001, 0.0),
        "l" | "liters" | "litres" => ("volume", 1.0, 0.0),
        "gal" => ("volume", 3.785411784, 0.0),
        "b" | "bytes" => ("data", 1.0, 0.0),
        "kb" => ("data", 1e3, 0.0),
        "mb" => ("data", 1e6, 0.0),
        "gb" => ("data", 1e9, 0.0),
        "tb" => ("data", 1e12, 0.0),
        "kib" => ("data", 1024.0, 0.0),
        "mib" => ("data", 1048576.0, 0.0),
        "gib" => ("data", 1073741824.0, 0.0),
        "c" | "°c" | "celsius" => ("temperature", 1.0, 273.15),
        "f" | "°f" | "fahrenheit" => ("temperature", 5.0 / 9.0, 255.3722222222222),
        "k" | "kelvin" => ("temperature", 1.0, 0.0),
        "deg" | "degrees" => ("angle", std::f64::consts::PI / 180.0, 0.0),
        "rad" | "radians" => ("angle", 1.0, 0.0),
        _ => return None,
    })
}
pub fn calculate(input: &str) -> Result<Option<Answer>, String> {
    let input = input.trim().trim_start_matches('=').trim();
    if input.is_empty() {
        return Ok(None);
    }
    if input.len() > 512 {
        return Err("Expression is too long (maximum 512 characters).".into());
    }
    let normalized = input.replace('×', "*").replace('÷', "/").replace('−', "-");
    let conversion = normalized
        .rsplit_once(" to ")
        .or_else(|| normalized.rsplit_once(" in "));
    if let Some((left, target)) = conversion {
        let target = target.trim();
        let (expression, source) = left
            .trim()
            .rfind(|c: char| c.is_ascii_digit() || c == ')' || c == '.')
            .map(|i| left.split_at(i + 1))
            .ok_or("Try a conversion like 10 km to mi.")?;
        let source = source.trim();
        let from = unit(source).ok_or_else(|| format!("Unknown unit: {source}"))?;
        let to = unit(target).ok_or_else(|| format!("Unknown unit: {target}"))?;
        if from.0 != to.0 {
            return Err("These units measure different things.".into());
        }
        let value = evaluate(expression)?;
        if from.0 == "temperature" && value * from.1 + from.2 < -1e-10 {
            return Err("Temperature is below absolute zero.".into());
        }
        return finite((value * from.1 + from.2 - to.2) / to.1, target.to_owned()).map(Some);
    }
    let looks_like_math = normalized
        .starts_with(|c: char| c.is_ascii_digit() || "(-+.".contains(c))
        || [
            "sqrt(", "sin(", "cos(", "tan(", "ln(", "log(", "abs(", "pi", "e^",
        ]
        .iter()
        .any(|s| normalized.starts_with(s));
    if !looks_like_math {
        return Ok(None);
    }
    finite(evaluate(&normalized)?, String::new()).map(Some)
}
fn evaluate(expression: &str) -> Result<f64, String> {
    meval::eval_str(expression.trim())
        .map_err(|_| "Check the expression, e.g. (24 + 8) / 2 or sqrt(144).".into())
}
fn finite(value: f64, unit: String) -> Result<Answer, String> {
    if !value.is_finite() {
        Err("The result is undefined or too large.".into())
    } else {
        Ok(Answer { value, unit })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn value(s: &str) -> f64 {
        calculate(s).unwrap().unwrap().value
    }
    #[test]
    fn arithmetic() {
        assert_eq!(value("(24 + 8) / 2"), 16.0);
        assert_eq!(value("2^3 + sqrt(144)"), 20.0);
        assert_eq!(value("−2 × 3"), -6.0);
    }
    #[test]
    fn conversion() {
        assert!((value("10km to mi") - 6.213711922).abs() < 1e-8);
        assert_eq!(value("(2+3) kg to g"), 5000.0);
        assert_eq!(value("1 GiB to MiB"), 1024.0);
        assert!((value("32 f to c")).abs() < 1e-10);
        assert!((value("100 c to f") - 212.0).abs() < 1e-10);
    }
    #[test]
    fn errors() {
        for s in [
            "1 / 0",
            "sqrt(-1)",
            "1 kg to km",
            "10 xyz to m",
            "-274 c to f",
            "(2+",
        ] {
            assert!(calculate(s).is_err(), "{s}");
        }
        assert_eq!(calculate("report.pdf"), Ok(None));
    }
    #[test]
    fn formatting() {
        assert_eq!(calculate("1/2").unwrap().unwrap().display(), "0.5");
        assert_eq!(calculate("0").unwrap().unwrap().display(), "0");
        assert!(calculate("1e-12").unwrap().unwrap().display().contains('e'));
    }
}
