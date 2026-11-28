//! Node-compatible request bytes, separated from the private logical response URL.
use super::*;

pub(super) fn header(value: &str) -> Result<ureq::http::HeaderValue, Error> {
    let bytes = value
        .chars()
        .map(|character| {
            u8::try_from(u32::from(character)).map_err(|_| Error::from("HTTP_HEADER_INVALID"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    ureq::http::HeaderValue::from_bytes(&bytes).map_err(|_| Error::from("HTTP_HEADER_INVALID"))
}
pub(super) fn absolute(value: &str) -> Result<Url, Error> {
    if value.len() > 4096 {
        return Err("HTTP_URL_INVALID".into());
    }
    let url = Url::parse(value).map_err(|_| Error::from("HTTP_URL_INVALID"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || url.as_str().len() > 4096
    {
        return Err("HTTP_URL_INVALID".into());
    }
    authorization(&url)?;
    Ok(url)
}
pub(super) fn transport_url(url: &Url) -> Url {
    let mut transport = url.clone();
    transport.set_fragment(None);
    transport
        .set_username("")
        .expect("HTTP URLs have an authority");
    transport
        .set_password(None)
        .expect("HTTP URLs have an authority");
    transport
}
pub(super) fn authorization(url: &Url) -> Result<Option<ureq::http::HeaderValue>, Error> {
    use base64::Engine as _;
    if url.username().is_empty() && url.password().unwrap_or_default().is_empty() {
        return Ok(None);
    }
    let user = component(url.username())?;
    let password = component(url.password().unwrap_or_default())?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"));
    Ok(Some(header(&format!("Basic {encoded}"))?))
}
fn component(value: &str) -> Result<String, Error> {
    let mut bytes = vec![];
    let mut input = value.as_bytes().iter().copied();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            };
            let high = input.next().and_then(digit).ok_or("HTTP_URL_INVALID")?;
            let low = input.next().and_then(digit).ok_or("HTTP_URL_INVALID")?;
            bytes.push(high * 16 + low);
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes).map_err(|_| Error::from("HTTP_URL_INVALID"))
}
