#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DnsError {
    Malformed,
    Unsupported,
    ResponseTooSmall,
}

/// Build a minimal authoritative A response pointing at the captive portal.
pub fn captive_response(
    request: &[u8],
    response: &mut [u8],
    address: [u8; 4],
) -> Result<usize, DnsError> {
    if request.len() < 12 || response.len() < 28 {
        return Err(DnsError::Malformed);
    }
    if request[2] & 0x80 != 0 || u16::from_be_bytes([request[4], request[5]]) != 1 {
        return Err(DnsError::Unsupported);
    }

    let mut cursor = 12;
    loop {
        let length = *request.get(cursor).ok_or(DnsError::Malformed)? as usize;
        cursor += 1;
        if length == 0 {
            break;
        }
        if length > 63 || cursor + length > request.len() {
            return Err(DnsError::Malformed);
        }
        cursor += length;
    }
    if cursor + 4 > request.len() {
        return Err(DnsError::Malformed);
    }
    let question_end = cursor + 4;
    let query_type = u16::from_be_bytes([request[cursor], request[cursor + 1]]);
    let query_class = u16::from_be_bytes([request[cursor + 2], request[cursor + 3]]);
    if query_type != 1 || query_class != 1 {
        return Err(DnsError::Unsupported);
    }
    let required = question_end + 16;
    if response.len() < required {
        return Err(DnsError::ResponseTooSmall);
    }

    response[..question_end].copy_from_slice(&request[..question_end]);
    response[2..4].copy_from_slice(&0x8580_u16.to_be_bytes());
    response[6..8].copy_from_slice(&1_u16.to_be_bytes());
    response[8..12].fill(0);
    let answer = &mut response[question_end..required];
    answer[..2].copy_from_slice(&0xc00c_u16.to_be_bytes());
    answer[2..4].copy_from_slice(&1_u16.to_be_bytes());
    answer[4..6].copy_from_slice(&1_u16.to_be_bytes());
    answer[6..10].copy_from_slice(&60_u32.to_be_bytes());
    answer[10..12].copy_from_slice(&4_u16.to_be_bytes());
    answer[12..16].copy_from_slice(&address);
    Ok(required)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_an_a_query() {
        let query = [
            0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0, 7, b'e', b'x', b'a', b'm', b'p', b'l',
            b'e', 3, b'c', b'o', b'm', 0, 0, 1, 0, 1,
        ];
        let mut response = [0; 64];
        let length = captive_response(&query, &mut response, [192, 168, 4, 1]).unwrap();
        assert_eq!(&response[..2], &[0x12, 0x34]);
        assert_eq!(&response[length - 4..length], &[192, 168, 4, 1]);
    }
}
