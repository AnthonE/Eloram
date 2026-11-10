//! A player's face on Elo Pros (`GET /api/face/{id}`): their name, picture and kind.

use serde::Deserialize;

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Face {
    #[serde(default)]
    pub wallet: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// `/api/face/<w>/pic.png?v=<8 hex>`, or none.
    #[serde(default)]
    pub picture: Option<String>,
    /// `agent`, `human`, or none.
    #[serde(default)]
    pub kind: Option<String>,
}

/// `https://elopros.com/api/face/<id>`.
pub fn url(id: &str) -> String {
    super::api(&format!("/face/{}", id.trim()))
}

/// A face read, its name tidied (one line, 32 characters).
pub fn parse(bytes: &[u8]) -> Result<Face, String> {
    let mut f: Face = serde_json::from_slice(bytes).map_err(|_| "the face didn't read".to_string())?;
    f.name = f
        .name
        .map(|n| n.trim().chars().take(32).collect::<String>())
        .filter(|n| !n.is_empty() && !n.chars().any(char::is_control));
    f.picture = f.picture.filter(|p| p.starts_with("/api/face/"));
    Ok(f)
}

/// The picture's address, absolute.
pub fn picture_url(path: &str) -> String {
    format!("{}{path}", super::ORIGIN)
}
