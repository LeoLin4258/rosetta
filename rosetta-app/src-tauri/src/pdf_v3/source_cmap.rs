use std::{collections::BTreeMap, fmt};

const MAX_RANGE_ENTRIES: u32 = 65_536;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToUnicodeMap {
    mappings: BTreeMap<Vec<u8>, String>,
    code_lengths: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToUnicodeDecodedUnit {
    pub text: String,
    pub encoded_start: usize,
    pub encoded_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToUnicodeError(String);

impl fmt::Display for ToUnicodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ToUnicodeError {}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Hex(Vec<u8>),
    Word(String),
    ArrayStart,
    ArrayEnd,
}

impl ToUnicodeMap {
    pub(crate) fn parse(content: &[u8]) -> Result<Self, ToUnicodeError> {
        let tokens = tokenize(content)?;
        if tokens
            .iter()
            .any(|token| matches!(token, Token::Word(word) if word == "usecmap"))
        {
            return Err(error("inherited ToUnicode CMaps are not supported"));
        }

        let mut mappings = BTreeMap::new();
        let mut code_lengths = Vec::new();
        let mut code_spaces = Vec::new();
        let mut index = 0usize;
        while index < tokens.len() {
            let Token::Word(operator) = &tokens[index] else {
                index += 1;
                continue;
            };
            match operator.as_str() {
                "begincodespacerange" => {
                    index += 1;
                    while !is_word(tokens.get(index), "endcodespacerange") {
                        let lower = expect_hex(&tokens, &mut index, "codespace lower bound")?;
                        let upper = expect_hex(&tokens, &mut index, "codespace upper bound")?;
                        if lower.is_empty() || lower.len() > 4 || lower.len() != upper.len() {
                            return Err(error("invalid ToUnicode codespace width"));
                        }
                        if lower > upper {
                            return Err(error("reversed ToUnicode codespace range"));
                        }
                        code_lengths.push(lower.len());
                        code_spaces.push((lower, upper));
                    }
                    index += 1;
                }
                "beginbfchar" => {
                    index += 1;
                    while !is_word(tokens.get(index), "endbfchar") {
                        let source = expect_hex(&tokens, &mut index, "bfchar source")?;
                        let destination = expect_hex(&tokens, &mut index, "bfchar destination")?;
                        insert_mapping(&mut mappings, source, decode_utf16_be(&destination)?)?;
                    }
                    index += 1;
                }
                "beginbfrange" => {
                    index += 1;
                    while !is_word(tokens.get(index), "endbfrange") {
                        let lower = expect_hex(&tokens, &mut index, "bfrange lower bound")?;
                        let upper = expect_hex(&tokens, &mut index, "bfrange upper bound")?;
                        let (lower_value, upper_value) = source_range(&lower, &upper)?;
                        let count = upper_value - lower_value + 1;
                        if count > MAX_RANGE_ENTRIES {
                            return Err(error("ToUnicode bfrange exceeds the safety limit"));
                        }
                        match tokens.get(index) {
                            Some(Token::Hex(destination)) => {
                                let destination = destination.clone();
                                index += 1;
                                for offset in 0..count {
                                    let source =
                                        fixed_width_bytes(lower_value + offset, lower.len());
                                    let destination = increment_big_endian(&destination, offset)?;
                                    insert_mapping(
                                        &mut mappings,
                                        source,
                                        decode_utf16_be(&destination)?,
                                    )?;
                                }
                            }
                            Some(Token::ArrayStart) => {
                                index += 1;
                                for offset in 0..count {
                                    let destination = expect_hex(
                                        &tokens,
                                        &mut index,
                                        "bfrange array destination",
                                    )?;
                                    insert_mapping(
                                        &mut mappings,
                                        fixed_width_bytes(lower_value + offset, lower.len()),
                                        decode_utf16_be(&destination)?,
                                    )?;
                                }
                                if !matches!(tokens.get(index), Some(Token::ArrayEnd)) {
                                    return Err(error("bfrange destination array length mismatch"));
                                }
                                index += 1;
                            }
                            _ => return Err(error("invalid bfrange destination")),
                        }
                    }
                    index += 1;
                }
                _ => index += 1,
            }
        }

        code_lengths.sort_unstable();
        code_lengths.dedup();
        code_lengths.reverse();
        if code_lengths.is_empty() {
            return Err(error("ToUnicode CMap has no codespace range"));
        }
        if mappings.is_empty() {
            return Err(error("ToUnicode CMap has no Unicode mappings"));
        }
        if mappings.keys().any(|source| {
            !code_spaces
                .iter()
                .any(|(lower, upper)| lower <= source && source <= upper)
        }) {
            return Err(error("ToUnicode mapping is outside its codespace"));
        }
        Ok(Self {
            mappings,
            code_lengths,
        })
    }

    pub(crate) fn decode(&self, encoded: &[u8]) -> Result<String, ToUnicodeError> {
        Ok(self
            .decode_units(encoded)?
            .into_iter()
            .map(|unit| unit.text)
            .collect())
    }

    pub(crate) fn decode_units(
        &self,
        encoded: &[u8],
    ) -> Result<Vec<ToUnicodeDecodedUnit>, ToUnicodeError> {
        let mut decoded = Vec::new();
        let mut offset = 0usize;
        while offset < encoded.len() {
            let mapping = self.code_lengths.iter().find_map(|length| {
                let end = offset.checked_add(*length)?;
                let source = encoded.get(offset..end)?;
                self.mappings.get(source).map(|value| (*length, value))
            });
            let Some((length, value)) = mapping else {
                return Err(error(format!(
                    "ToUnicode CMap has no mapping at byte offset {offset}"
                )));
            };
            decoded.push(ToUnicodeDecodedUnit {
                text: value.clone(),
                encoded_start: offset,
                encoded_len: length,
            });
            offset += length;
        }
        Ok(decoded)
    }
}

fn tokenize(content: &[u8]) -> Result<Vec<Token>, ToUnicodeError> {
    let mut tokens = Vec::new();
    let mut index = 0usize;
    while index < content.len() {
        match content[index] {
            byte if byte.is_ascii_whitespace() => index += 1,
            b'%' => {
                index += 1;
                while index < content.len() && !matches!(content[index], b'\r' | b'\n') {
                    index += 1;
                }
            }
            b'<' if content.get(index + 1) == Some(&b'<') => index += 2,
            b'>' if content.get(index + 1) == Some(&b'>') => index += 2,
            b'<' => {
                index += 1;
                let mut nibbles = Vec::new();
                while index < content.len() && content[index] != b'>' {
                    if content[index].is_ascii_hexdigit() {
                        nibbles.push(content[index]);
                    } else if !content[index].is_ascii_whitespace() {
                        return Err(error("invalid byte in ToUnicode hexadecimal string"));
                    }
                    index += 1;
                }
                if index == content.len() {
                    return Err(error("unterminated ToUnicode hexadecimal string"));
                }
                index += 1;
                if nibbles.len() % 2 == 1 {
                    nibbles.push(b'0');
                }
                let mut bytes = Vec::with_capacity(nibbles.len() / 2);
                for pair in nibbles.chunks_exact(2) {
                    bytes.push((hex_value(pair[0])? << 4) | hex_value(pair[1])?);
                }
                tokens.push(Token::Hex(bytes));
            }
            b'[' => {
                tokens.push(Token::ArrayStart);
                index += 1;
            }
            b']' => {
                tokens.push(Token::ArrayEnd);
                index += 1;
            }
            b'(' => skip_literal_string(content, &mut index)?,
            byte if is_delimiter(byte) => index += 1,
            _ => {
                let start = index;
                while index < content.len()
                    && !content[index].is_ascii_whitespace()
                    && !is_delimiter(content[index])
                {
                    index += 1;
                }
                tokens.push(Token::Word(
                    String::from_utf8_lossy(&content[start..index]).into_owned(),
                ));
            }
        }
    }
    Ok(tokens)
}

fn skip_literal_string(content: &[u8], index: &mut usize) -> Result<(), ToUnicodeError> {
    *index += 1;
    let mut depth = 1usize;
    while *index < content.len() {
        match content[*index] {
            b'\\' => *index = (*index + 2).min(content.len()),
            b'(' => {
                depth += 1;
                *index += 1;
            }
            b')' => {
                depth -= 1;
                *index += 1;
                if depth == 0 {
                    return Ok(());
                }
            }
            _ => *index += 1,
        }
    }
    Err(error("unterminated ToUnicode literal string"))
}

fn is_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'<' | b'>' | b'[' | b']' | b'(' | b')' | b'{' | b'}' | b'/'
    )
}

fn hex_value(byte: u8) -> Result<u8, ToUnicodeError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(error("invalid ToUnicode hexadecimal digit")),
    }
}

fn expect_hex(
    tokens: &[Token],
    index: &mut usize,
    context: &str,
) -> Result<Vec<u8>, ToUnicodeError> {
    let Some(Token::Hex(value)) = tokens.get(*index) else {
        return Err(error(format!("missing {context}")));
    };
    *index += 1;
    Ok(value.clone())
}

fn is_word(token: Option<&Token>, expected: &str) -> bool {
    matches!(token, Some(Token::Word(word)) if word == expected)
}

fn source_range(lower: &[u8], upper: &[u8]) -> Result<(u32, u32), ToUnicodeError> {
    if lower.is_empty() || lower.len() > 4 || lower.len() != upper.len() {
        return Err(error("invalid ToUnicode bfrange source width"));
    }
    let lower = bytes_to_u32(lower);
    let upper = bytes_to_u32(upper);
    if lower > upper {
        return Err(error("reversed ToUnicode bfrange"));
    }
    Ok((lower, upper))
}

fn bytes_to_u32(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0u32, |value, byte| (value << 8) | u32::from(*byte))
}

fn fixed_width_bytes(value: u32, width: usize) -> Vec<u8> {
    value.to_be_bytes()[4 - width..].to_vec()
}

fn increment_big_endian(bytes: &[u8], increment: u32) -> Result<Vec<u8>, ToUnicodeError> {
    let mut result = bytes.to_vec();
    let mut carry = increment;
    for byte in result.iter_mut().rev() {
        let value = u32::from(*byte) + (carry & 0xff);
        *byte = value as u8;
        carry = (carry >> 8) + (value >> 8);
    }
    if carry != 0 {
        return Err(error("ToUnicode bfrange destination overflow"));
    }
    Ok(result)
}

fn decode_utf16_be(bytes: &[u8]) -> Result<String, ToUnicodeError> {
    if bytes.is_empty() || bytes.len() % 2 != 0 {
        return Err(error("invalid UTF-16BE ToUnicode destination"));
    }
    let units = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    String::from_utf16(&units).map_err(|_| error("invalid UTF-16 ToUnicode destination"))
}

fn insert_mapping(
    mappings: &mut BTreeMap<Vec<u8>, String>,
    source: Vec<u8>,
    destination: String,
) -> Result<(), ToUnicodeError> {
    if mappings.insert(source, destination).is_some() {
        return Err(error("duplicate source code in ToUnicode CMap"));
    }
    Ok(())
}

fn error(message: impl Into<String>) -> ToUnicodeError {
    ToUnicodeError(message.into())
}

#[cfg(test)]
mod tests {
    use super::ToUnicodeMap;

    #[test]
    fn decodes_one_byte_bfchar_and_ligatures() {
        let map = ToUnicodeMap::parse(
            br#"1 begincodespacerange
<00> <FF>
endcodespacerange
2 beginbfchar
<01> <0041>
<02> <00660069>
endbfchar"#,
        )
        .expect("one-byte CMap");

        assert_eq!(map.decode(&[1, 2]).expect("decode"), "Afi");
        assert_eq!(
            map.decode_units(&[1, 2]).expect("decode units"),
            vec![
                super::ToUnicodeDecodedUnit {
                    text: "A".to_string(),
                    encoded_start: 0,
                    encoded_len: 1,
                },
                super::ToUnicodeDecodedUnit {
                    text: "fi".to_string(),
                    encoded_start: 1,
                    encoded_len: 1,
                },
            ]
        );
    }

    #[test]
    fn decodes_incrementing_and_array_bfranges() {
        let map = ToUnicodeMap::parse(
            br#"1 begincodespacerange
<00> <FF>
endcodespacerange
2 beginbfrange
<20> <22> <0061>
<30> <31> [<03B1> <03B2>]
endbfrange"#,
        )
        .expect("range CMap");

        assert_eq!(
            map.decode(&[0x20, 0x21, 0x22, 0x30, 0x31]).expect("decode"),
            "abcαβ"
        );
    }

    #[test]
    fn decodes_two_byte_sources_and_utf16_surrogate_pairs() {
        let map = ToUnicodeMap::parse(
            br#"1 begincodespacerange
<0000> <FFFF>
endcodespacerange
1 beginbfchar
<0042> <D83DDE00>
endbfchar"#,
        )
        .expect("two-byte CMap");

        assert_eq!(map.decode(&[0, 0x42]).expect("decode"), "😀");
    }

    #[test]
    fn rejects_unmapped_source_codes() {
        let map = ToUnicodeMap::parse(
            br#"1 begincodespacerange
<00> <FF>
endcodespacerange
1 beginbfchar
<01> <0041>
endbfchar"#,
        )
        .expect("CMap");

        assert!(map.decode(&[2]).is_err());
    }

    #[test]
    fn rejects_mappings_outside_the_declared_codespace() {
        let result = ToUnicodeMap::parse(
            br#"1 begincodespacerange
<00> <7F>
endcodespacerange
1 beginbfchar
<80> <0041>
endbfchar"#,
        );

        assert!(result.is_err());
    }
}
