use std::fs;

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VersionMeta {
    pub id: String,
    #[serde(rename = "inheritsFrom")]
    pub inherits_from: Option<String>,
}

fn main() {
    let content = fs::read_to_string(r"C:\Users\MaoZa\.dawnland\.minecraft\versions\neoforge-21.1.228\neoforge-21.1.228.json").unwrap();
    let meta: VersionMeta = serde_json::from_str(&content).unwrap();
    println!("Parsed meta: {:?}", meta);
}
