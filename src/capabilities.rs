//! Parsing of MCCS capabilities strings, the self-description a monitor sends over DDC/CI.

/// The allowed values the monitor lists for one VCP code, e.g. the inputs for 0x60.
/// `None` when the code isn't listed or lists no values.
pub fn vcp_values(caps: &str, code: u8) -> Option<Vec<u32>> {
    let vcp = section(caps, "vcp")?;
    let mut chars = vcp.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_hexdigit() {
            let token: String = std::iter::from_fn(|| chars.next_if(char::is_ascii_hexdigit)).collect();
            while chars.next_if(|c| *c == ' ').is_some() {}
            if chars.next_if(|c| *c == '(').is_some() {
                let values: String = std::iter::from_fn(|| chars.next_if(|c| *c != ')')).collect();
                chars.next();
                if u8::from_str_radix(&token, 16) == Ok(code) {
                    let values: Vec<u32> = values
                        .split_whitespace()
                        .filter_map(|v| u32::from_str_radix(v, 16).ok())
                        .collect();
                    return (!values.is_empty()).then_some(values);
                }
            }
        } else {
            chars.next();
        }
    }
    None
}

/// The contents of `name(...)`, with nested parentheses kept.
fn section<'a>(caps: &'a str, name: &str) -> Option<&'a str> {
    let start = caps.find(&format!("{name}("))? + name.len() + 1;
    let mut depth = 1;
    for (i, c) in caps[start..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&caps[start..start + i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Display name for an input source value (VCP 0x60), per MCCS 2.2.
pub fn input_name(value: u32) -> String {
    match value {
        0x01 => "VGA 1".into(),
        0x02 => "VGA 2".into(),
        0x03 => "DVI 1".into(),
        0x04 => "DVI 2".into(),
        0x0F => "DisplayPort 1".into(),
        0x10 => "DisplayPort 2".into(),
        0x11 => "HDMI 1".into(),
        0x12 => "HDMI 2".into(),
        0x1B => "USB-C".into(),
        other => format!("Input {other:#04X}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Read from an LG HDR 4K and an Acer VG240Y V with examples/probe.rs
    const LG: &str = "(prot(monitor)type(lcd)UP550cmds(01 02 03 0C E3 F3)vcp(02 04 05 08 10 12 14(05 08 0B ) 16 18 1A 52 60( 11 12 0F 00) AC AE B2 B6 C0 C6 C8 C9 D6(01 04) DF 62 8D F4 F5(00 01 02) F6(00 01 02) 4D 4E 4F 15(01 06 11 13 14 15 18 19 28 29 48) F7(00 01 02 03) F8(00 01) F9 E4 E5 E6 E7 E8 E9 EA EB EF FD(00 01) FE(00 01 02) FF)mccs_ver(2.1)mswhql(1))";
    const ACER: &str = "(prot(monitor)type(lcd)model(XV240YV)cmds(01 02 03 07 0C E3 F3)vcp(02 04 05 08 0B 0C 10 12 14(05 06 08 0B) 16 18 1A 52 54(00 01) 60(0F 11 12) 59 5A 5B 5C 5D 5E 9B 9C 9D 9E 9F A0 AC AE B6 C0 C6 C8 C9 CC(01 02 03 04 05 06 08 09 0A 0C 0D 0E 14 16 1E)D6(01 04 05) DF E0(00 03 04 05 06) E1(00 01 02 03) E2(00 01 02 03 05 06 07 0B 10 11 12) E3 E4 E5 E7(00 01 02) E8(00 01 02 03 04) )mswhql(1)asset_eep(32)mccs_ver(2.1))";

    #[test]
    fn inputs() {
        assert_eq!(vcp_values(LG, 0x60), Some(vec![0x11, 0x12, 0x0F, 0x00]));
        assert_eq!(vcp_values(ACER, 0x60), Some(vec![0x0F, 0x11, 0x12]));
    }

    #[test]
    fn values_right_after_a_list() {
        // No space between CC(...) and D6 on the Acer
        assert_eq!(vcp_values(ACER, 0xD6), Some(vec![0x01, 0x04, 0x05]));
    }

    #[test]
    fn codes_without_values() {
        assert_eq!(vcp_values(LG, 0x62), None);
        assert_eq!(vcp_values(ACER, 0x62), None);
        assert_eq!(vcp_values("(prot(monitor))", 0x60), None);
    }
}
