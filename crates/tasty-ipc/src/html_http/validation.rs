use super::*;

fn token(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(b))
}
fn headers(values: &[Header]) -> Result<()> {
    if values.len() > HEADER_COUNT {
        return Err("HTTP header count");
    }
    let mut total = 0usize;
    for h in values {
        total = total
            .checked_add(h.name.len())
            .and_then(|n| n.checked_add(h.value.len()))
            .ok_or("HTTP header overflow")?;
        if total > HEADER_BYTES
            || !token(h.name.as_bytes())
            || h.value
                .iter()
                .any(|b| *b != b'\t' && (*b < 32 || *b == 127))
        {
            return Err("HTTP header bounds or syntax");
        }
    }
    Ok(())
}
fn url(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > URL_BYTES || value.bytes().any(|b| b <= 32 || b == 127) {
        return Err("HTTP URL bounds or syntax");
    }
    Ok(())
}
fn window(offset: u64, n: u32) -> Result<()> {
    if n == 0 || n as usize > CHUNK_BYTES {
        return Err("HTTP credit window");
    }
    usize::try_from(offset).map_err(|_| "HTTP offset address space")?;
    let end = offset
        .checked_add(u64::from(n))
        .ok_or("HTTP offset overflow")?;
    usize::try_from(end).map_err(|_| "HTTP window address space")?;
    Ok(())
}
fn response(id: ResponseId) -> Result<()> {
    if id.serial == 0 {
        return Err("HTTP response serial");
    }
    Ok(())
}
fn identities(id: TransferId, generation: u64, r: Option<ResponseId>) -> Result<()> {
    if let Some(r) = r {
        response(r)?;
        if r.request != id || r.source_generation != generation {
            return Err("HTTP response identity");
        }
    }
    Ok(())
}
/// Strict canonical RFC4648 chunks. Bound encoded allocation before decoding.
pub fn decode_chunk(value: &str) -> Result<Vec<u8>> {
    if value.len() > CHUNK_BYTES.div_ceil(3) * 4 || !value.len().is_multiple_of(4) {
        return Err("HTTP base64 length");
    }
    let mut out = Vec::with_capacity(value.len() / 4 * 3);
    fn sextet(b: u8) -> Result<u8> {
        match b {
            b'A'..=b'Z' => Ok(b - b'A'),
            b'a'..=b'z' => Ok(b - b'a' + 26),
            b'0'..=b'9' => Ok(b - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err("HTTP base64 syntax"),
        }
    }
    for (i, c) in value.as_bytes().chunks_exact(4).enumerate() {
        let a = sextet(c[0])?;
        let b = sextet(c[1])?;
        let last = (i + 1) * 4 == value.len();
        out.push((a << 2) | (b >> 4));
        if c[2] == b'=' {
            if !last || c[3] != b'=' || b & 15 != 0 {
                return Err("HTTP base64 padding");
            }
        } else {
            let d = sextet(c[2])?;
            out.push((b << 4) | (d >> 2));
            if c[3] == b'=' {
                if !last || d & 3 != 0 {
                    return Err("HTTP base64 padding");
                }
            } else {
                out.push((d << 6) | sextet(c[3])?);
            }
        }
    }
    if out.len() > CHUNK_BYTES {
        return Err("HTTP decoded chunk limit");
    }
    Ok(out)
}
fn chunk(offset: u64, data: &str) -> Result<()> {
    let bytes = decode_chunk(data)?;
    let end = offset
        .checked_add(u64::try_from(bytes.len()).map_err(|_| "HTTP chunk length")?)
        .ok_or("HTTP chunk overflow")?;
    usize::try_from(end).map_err(|_| "HTTP chunk address space")?;
    Ok(())
}
fn begin_values(
    method: &str,
    logical_url: &str,
    hs: &[Header],
    upload: BodyIdentity,
) -> Result<()> {
    if method.len() > METHOD_BYTES || !token(method.as_bytes()) {
        return Err("HTTP method bounds or syntax");
    }
    url(logical_url)?;
    headers(hs)?;
    upload.checked_len()?;
    Ok(())
}
impl HttpRequest {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Reload { previous, begin } => {
                response(*previous)?;
                let a = previous.request;
                let b = begin.id;
                if (a.epoch, a.surface_id, a.binding, previous.source_generation)
                    != (b.epoch, b.surface_id, b.binding, begin.source_generation)
                    || a.document >= b.document
                    || a.request >= b.request
                    || a.transfer >= b.transfer
                {
                    return Err("successor scope or order");
                }
                begin_values(
                    &begin.method,
                    &begin.logical_url,
                    &begin.headers,
                    begin.upload,
                )?;
            }
            Self::Begin {
                method,
                logical_url,
                headers: hs,
                upload,
                ..
            } => {
                begin_values(method, logical_url, hs, *upload)?;
            }
            Self::Upload {
                offset, data_b64, ..
            } => chunk(*offset, data_b64)?,
            Self::Credit {
                response: r,
                offset,
                max_bytes,
                ..
            } => {
                response(*r)?;
                window(*offset, *max_bytes)?;
            }
            Self::Reuse { response: r } | Self::Release { response: r } => response(*r)?,
            _ => {}
        }
        // Check the entire escaped envelope, not merely decoded payload lengths.
        frame(self).map(|_| ())
    }
}
impl HttpReply {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::UploadReady {
                offset, max_bytes, ..
            } => window(*offset, *max_bytes)?,
            Self::Head { head } => {
                response(head.id)?;
                url(&head.logical_url)?;
                headers(&head.headers)?;
                if !(100..=599).contains(&head.status) {
                    return Err("HTTP response status");
                }
                if let Some(n) = head.encoded_length {
                    usize::try_from(n).map_err(|_| "HTTP length address space")?;
                }
            }
            Self::AllowanceRequired {
                response: r,
                encoded_at_least,
                known_length,
            } => {
                response(*r)?;
                if *encoded_at_least <= AUTO_BYTES
                    || known_length.is_some_and(|n| n < *encoded_at_least)
                {
                    return Err("HTTP allowance bounds");
                }
                usize::try_from(*encoded_at_least).map_err(|_| "HTTP allowance address space")?;
                if let Some(n) = known_length {
                    usize::try_from(*n).map_err(|_| "HTTP length address space")?;
                }
            }
            Self::Chunk {
                response: r,
                offset,
                data_b64,
            } => {
                response(*r)?;
                chunk(*offset, data_b64)?;
            }
            Self::Complete {
                response: r,
                encoded,
                decoded,
            } => {
                response(*r)?;
                encoded.checked_len()?;
                decoded.checked_len()?;
            }
            Self::Failure {
                id,
                source_generation,
                response: r,
                reason,
                ..
            }
            | Self::CommandRejected {
                id,
                source_generation,
                response: r,
                reason,
                ..
            } => {
                identities(*id, *source_generation, *r)?;
                if reason.len() > REASON_BYTES || reason.chars().any(char::is_control) {
                    return Err("HTTP failure reason");
                }
            }
            Self::Cancelled {
                id,
                source_generation,
                response: r,
            } => identities(*id, *source_generation, *r)?,
        }
        frame(self).map(|_| ())
    }
}
