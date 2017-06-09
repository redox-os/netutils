#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Default)]
pub struct MacAddr {
    pub bytes: [u8; 6],
}

impl MacAddr {
    pub const BROADCAST: MacAddr = MacAddr { bytes: [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF] };

    pub fn from_str(string: &str) -> Self {
        MacAddr::try_parse_with_delimeter(string, ':')
            .or_else(|| MacAddr::try_parse_with_delimeter(string, '-'))
            .unwrap_or_default()
    }

    fn try_parse_with_delimeter(string: &str, delimeter: char) -> Option<MacAddr> {
        let mut addr = MacAddr::default();
        let mut segments = 0;

        for part in string.split(delimeter) {
            if segments >= addr.bytes.len() {
                return None;
            }
            addr.bytes[segments] = match u8::from_str_radix(part, 16) {
                Ok(b) => b,
                _ => return None,
            };
            segments += 1;
        }

        if segments == addr.bytes.len() {
            Some(addr)
        } else {
            None
        }
    }

    pub fn to_string(&self) -> String {
        format!("{:>02X}-{:>02X}-{:>02X}-{:>02X}-{:>02X}-{:>02X}",
                self.bytes[0],
                self.bytes[1],
                self.bytes[2],
                self.bytes[3],
                self.bytes[4],
                self.bytes[5])
    }
}

#[cfg(test)]
mod test {
    use super::MacAddr;

    #[test]
    fn from_str_test() {
        let mac = MacAddr { bytes: [0x01, 0x23, 0x45, 0x67, 0x89, 0xab] };
        let empty_mac = MacAddr::default();

        assert_eq!(mac, MacAddr::from_str("01:23:45:67:89:ab"));
        assert_eq!(mac, MacAddr::from_str("1:23:45:67:89:ab"));
        assert_eq!(mac, MacAddr::from_str("01:23:45:67:89:AB"));
        assert_eq!(mac, MacAddr::from_str("01-23-45-67-89-ab"));
        assert_eq!(empty_mac, MacAddr::from_str(""));
        assert_eq!(empty_mac, MacAddr::from_str("01:23:45:67:89"));
        assert_eq!(empty_mac, MacAddr::from_str("01:23:45:67:89:ab:cd"));
        assert_eq!(empty_mac, MacAddr::from_str("x1:23:45:67:89:ab"));
        assert_eq!(empty_mac, MacAddr::from_str("01:23-45-67-89-ab"));
        assert_eq!(empty_mac, MacAddr::from_str("01-23-45-67-89-ag"));
        assert_eq!(empty_mac, MacAddr::from_str("01.23.45.67.89.ab"));
        assert_eq!(empty_mac, MacAddr::from_str("01234-23-45-67-89-ab"));
        assert_eq!(empty_mac, MacAddr::from_str("01--23-45-67-89-ab"));
        assert_eq!(empty_mac, MacAddr::from_str("12"));
        assert_eq!(empty_mac, MacAddr::from_str("0:0:0:0:0:0"));

        assert_eq!(mac, MacAddr::from_str(&mac.to_string()));
        assert_eq!(empty_mac, MacAddr::from_str(&empty_mac.to_string()));
    }
}
