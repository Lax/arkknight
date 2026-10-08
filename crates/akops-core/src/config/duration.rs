//! 人类可读时长（`"2h"` / `"90m"` / `"1h30m"` / `"45s"` / `"7d"`）的 serde 支持。

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// 包装 [`Duration`]，以 `2h`/`90m` 形态参与 TOML 序列化。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HumanDuration(pub Duration);

impl HumanDuration {
    /// 解析时长串：若干「数字+单位」段，单位 `s`/`m`/`h`/`d`。
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        if s.is_empty() {
            return Err("时长为空".into());
        }
        let mut total: u64 = 0;
        let mut num = String::new();
        for c in s.chars() {
            if c.is_ascii_digit() {
                num.push(c);
            } else {
                let n: u64 = num
                    .parse()
                    .map_err(|_| format!("时长 `{s}` 数字段 `{num}` 不合法"))?;
                num.clear();
                let mult = match c {
                    's' => 1,
                    'm' => 60,
                    'h' => 3600,
                    'd' => 86400,
                    _ => return Err(format!("时长 `{s}` 含未知单位 `{c}`（支持 s/m/h/d）")),
                };
                total = total
                    .checked_add(n.checked_mul(mult).ok_or("时长溢出")?)
                    .ok_or("时长溢出")?;
            }
        }
        if !num.is_empty() {
            return Err(format!("时长 `{s}` 结尾缺少单位（如 `2h`、`90m`）"));
        }
        if total == 0 {
            return Err(format!("时长 `{s}` 不能为 0"));
        }
        Ok(HumanDuration(Duration::from_secs(total)))
    }
}

impl FromStr for HumanDuration {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl fmt::Display for HumanDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let secs = self.0.as_secs();
        let (d, rem) = (secs / 86400, secs % 86400);
        let (h, rem) = (rem / 3600, rem % 3600);
        let (m, s) = (rem / 60, rem % 60);
        let mut parts: Vec<String> = Vec::new();
        if d > 0 {
            parts.push(format!("{d}d"));
        }
        if h > 0 {
            parts.push(format!("{h}h"));
        }
        if m > 0 {
            parts.push(format!("{m}m"));
        }
        if s > 0 || parts.is_empty() {
            parts.push(format!("{s}s"));
        }
        write!(f, "{}", parts.join(""))
    }
}

impl Serialize for HumanDuration {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for HumanDuration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_display() {
        assert_eq!(
            HumanDuration::parse("2h").unwrap().0,
            Duration::from_secs(7200)
        );
        assert_eq!(
            HumanDuration::parse("90m").unwrap().0,
            Duration::from_secs(5400)
        );
        assert_eq!(
            HumanDuration::parse("1h30m").unwrap().0,
            Duration::from_secs(5400)
        );
        assert_eq!(
            HumanDuration::parse("7d").unwrap().0,
            Duration::from_secs(604800)
        );
        assert_eq!(
            HumanDuration::parse(" 45s ").unwrap().0,
            Duration::from_secs(45)
        );

        for src in ["", "h", "2", "2x", "1h30", "0m"] {
            assert!(HumanDuration::parse(src).is_err(), "应拒绝 `{src}`");
        }
    }

    #[test]
    fn display_roundtrip() {
        for src in ["2h", "90m", "45s", "7d", "1h30m", "1d12h"] {
            let d = HumanDuration::parse(src).unwrap();
            assert_eq!(
                d.to_string(),
                HumanDuration::parse(&d.to_string()).unwrap().to_string(),
                "{src}"
            );
        }
        assert_eq!(HumanDuration::parse("5400s").unwrap().to_string(), "1h30m");
    }

    #[test]
    fn toml_serde() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Wrapper {
            v: HumanDuration,
        }
        let src = "v = \"6h\"\n";
        let w: Wrapper = toml::from_str(src).unwrap();
        assert_eq!(w.v.0, Duration::from_secs(21600));
        assert_eq!(toml::to_string(&w).unwrap(), src);
    }
}
