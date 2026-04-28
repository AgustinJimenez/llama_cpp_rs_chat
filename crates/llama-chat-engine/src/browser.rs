/// Legacy browser backend enum — kept for backward compatibility.
#[derive(Debug, Clone, PartialEq)]
pub enum BrowserBackend {
    None,
}

impl BrowserBackend {
    pub fn from_config(_s: Option<&str>) -> Self {
        Self::None
    }
}
