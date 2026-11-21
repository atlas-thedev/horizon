use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
}

impl Profile {
    pub fn label(&self) -> String {
        use horizon_client::lang::get_or as tr;
        match self.kind.as_str() {
            "offline" => tr("native.profile.offline", "Offline"),
            "microsoft" => "Microsoft".into(),
            _ => "Native".into(),
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profiles {
    nonce: String,
    pub active_id: Option<String>,
    pub profiles: Vec<Profile>,
}

impl Profiles {
    pub fn load(dir: &Path) -> Self {
        let file = dir.join(".native/profiles.json");
        (|| {
            if file.metadata().ok()?.len() > 65536 {
                return None;
            }
            serde_json::from_slice(&std::fs::read(file).ok()?).ok()
        })()
        .unwrap_or_default()
    }

    pub fn request(&self, dir: &Path, profile: &Profile) -> std::io::Result<()> {
        if self.nonce.is_empty() || self.active_id.as_deref() == Some(&profile.id) {
            return Err(std::io::ErrorKind::InvalidInput.into());
        }
        let folder = dir.join(".native");
        let file = folder.join("profile-request.json");
        let staged = folder.join("profile-request.json.tmp");
        let body = serde_json::json!({"nonce": self.nonce, "id": profile.id});
        std::fs::write(&staged, serde_json::to_vec(&body)?)?;
        if let Err(error) = std::fs::rename(&staged, file) {
            let _ = std::fs::remove_file(staged);
            return Err(error);
        }
        Ok(())
    }
}
