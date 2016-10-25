#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Ipv4Addr {
    pub bytes: [u8; 4],
}

impl Ipv4Addr {
    pub const BROADCAST: Ipv4Addr = Ipv4Addr { bytes: [255, 255, 255, 255] };
    pub const LOOPBACK: Ipv4Addr = Ipv4Addr { bytes: [127, 0, 0, 1] };
    pub const NULL: Ipv4Addr = Ipv4Addr { bytes: [0, 0, 0, 0] };

    pub fn from_str(string: &str) -> Self {
        let mut addr = Ipv4Addr { bytes: [0, 0, 0, 0] };

        let mut i = 0;
        for part in string.split('.') {
            let octet = part.parse::<u8>().unwrap_or(0);
            match i {
                0 => addr.bytes[0] = octet,
                1 => addr.bytes[1] = octet,
                2 => addr.bytes[2] = octet,
                3 => addr.bytes[3] = octet,
                _ => break,
            }
            i += 1;
        }

        addr
    }

    pub fn to_string(&self) -> String {
        let mut string = String::new();

        for i in 0..4 {
            if i > 0 {
                string = string + ".";
            }
            string = string + &format!("{}", self.bytes[i]);
        }

        string
    }
}
