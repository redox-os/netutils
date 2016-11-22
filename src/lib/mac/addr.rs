#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct MacAddr {
    pub bytes: [u8; 6],
}

impl MacAddr {
    pub const BROADCAST: MacAddr = MacAddr { bytes: [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF] };

    pub fn from_str(string: &str) -> Self {
        let mut addr = MacAddr { bytes: [0, 0, 0, 0, 0, 0] };

        let mut i = 0;
        for part in string.split('.') {
            let octet = u8::from_str_radix(part, 16).unwrap_or(0);
            match i {
                0 => addr.bytes[0] = octet,
                1 => addr.bytes[1] = octet,
                2 => addr.bytes[2] = octet,
                3 => addr.bytes[3] = octet,
                4 => addr.bytes[4] = octet,
                5 => addr.bytes[5] = octet,
                _ => break,
            }
            i += 1;
        }

        addr
    }

    pub fn to_string(&self) -> String {
        let mut string = String::new();
        for i in 0..6 {
            if i > 0 {
                string.push('.');
            }
            string.push_str(&format!("{:>02X}", self.bytes[i]));
        }
        string
    }
}
