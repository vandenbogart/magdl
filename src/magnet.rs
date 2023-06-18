pub struct Magnet {
    pub tracker_urls: Vec<url::Url>,
    pub info_hash: [u8; 20],
    pub display_name: String,
}
impl Magnet {
    pub fn from_link_string(value: &str) -> Self {
        let decoded = urlencoding::decode(&value).expect("Failed to parse magnet link");
        let slice = &decoded[8..];
        let split = slice.split("&").collect::<Vec<_>>();

        let mut trackers = Vec::new();
        let mut exact_topic = [0u8; 20];
        let mut display_name = String::new();
        for item in split {
            let (id, value) = item.split_once("=").unwrap();
            match id {
                "xt" => {
                    let info_string = value[value.len() - 40..].as_bytes();
                    let bytes = hex::decode(info_string)
                        .expect("Failed to parse info hash from magnet link");
                    exact_topic.copy_from_slice(bytes.as_slice());
                }
                "dn" => {
                    display_name = String::from(value);
                }
                "tr" => {
                    use std::str::FromStr;
                    if let Some(tracker) = url::Url::from_str(value).ok() {
                        trackers.push(tracker);
                    }
                }
                &_ => (),
            }
        }
        Self {
            tracker_urls: trackers,
            info_hash: exact_topic,
            display_name,
        }
    }
}
