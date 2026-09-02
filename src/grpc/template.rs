pub(crate) fn fake_value(field_name: &str, kind: &prost_reflect::Kind) -> serde_json::Value {
    use fake::Fake;
    use prost_reflect::Kind;

    let n = rand::random::<u32>() % 100000;

    match kind {
        Kind::Double | Kind::Float => serde_json::json!((n as f64) / 10.0),
        Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => serde_json::json!(n as i32),
        Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => serde_json::json!((n * 100) as i64),
        Kind::Uint32 | Kind::Fixed32 => serde_json::json!(n),
        Kind::Uint64 | Kind::Fixed64 => serde_json::json!((n * 100) as u64),
        Kind::Bool => serde_json::json!(n.is_multiple_of(2)),

        Kind::String => {
            let lower = field_name.to_lowercase();
            let val: String = if lower.contains("email") || lower.contains("mail") {
                fake::faker::internet::en::FreeEmail().fake()
            } else if lower.contains("name")
                && (lower.contains("first") || lower.starts_with("first"))
            {
                fake::faker::name::en::FirstName().fake()
            } else if lower.contains("name")
                && (lower.contains("last") || lower.contains("surname"))
            {
                fake::faker::name::en::LastName().fake()
            } else if lower.contains("name") {
                fake::faker::name::en::Name().fake()
            } else if lower.contains("phone") || lower.contains("tel") {
                fake::faker::phone_number::en::PhoneNumber().fake()
            } else if lower.contains("url") || lower.contains("uri") || lower.contains("link") {
                format!(
                    "https://example.com/{}",
                    fake::faker::lorem::en::Word().fake::<String>()
                )
            } else if lower.contains("uuid") || lower.contains("guid") {
                let u = uuid::Uuid::new_v4();
                u.to_string()
            } else if lower.contains("address") || lower.contains("street") {
                format!(
                    "{} {}",
                    fake::faker::address::en::StreetName().fake::<String>(),
                    rand::random::<u16>() % 10000 + 1
                )
            } else if lower.contains("city") {
                fake::faker::address::en::CityName().fake()
            } else if lower.contains("country") {
                fake::faker::address::en::CountryName().fake()
            } else if lower.contains("zip")
                || lower.contains("postal")
                || lower.contains("postcode")
            {
                fake::faker::address::en::PostCode().fake()
            } else if lower.contains("password") || lower.contains("secret") {
                "••••••••".to_string()
            } else if lower.contains("token") {
                format!("tok_{:x}", uuid::Uuid::new_v4().as_u128() >> 64)
            } else if lower.contains("description")
                || lower.contains("comment")
                || lower.contains("note")
                || lower.contains("bio")
            {
                fake::faker::lorem::en::Paragraph(3..6).fake()
            } else if lower.contains("sentence")
                || lower.contains("text")
                || lower.contains("content")
            {
                fake::faker::lorem::en::Sentence(3..8).fake()
            } else if lower.contains("status") {
                ["active", "inactive", "pending"][n as usize % 3].to_string()
            } else if lower.contains("type") || lower.contains("kind") || lower.contains("category")
            {
                ["standard", "premium", "basic"][n as usize % 3].to_string()
            } else if lower.contains("date")
                || lower.contains("time")
                || lower.contains("timestamp")
                || lower.ends_with("_at")
                || lower == "at"
            {
                "2024-06-15T10:30:00Z".to_string()
            } else if lower.contains("color") {
                ["#3b82f6", "#ef4444", "#22c55e", "#f59e0b"][n as usize % 4].to_string()
            } else if lower.contains("lang") || lower.contains("locale") {
                "en-US".to_string()
            } else if lower.contains("avatar")
                || lower.contains("image")
                || lower.contains("photo")
                || lower.contains("picture")
                || lower.contains("icon")
            {
                format!("https://i.pravatar.cc/150?u={}", n)
            } else if lower.contains("title")
                || lower.contains("subject")
                || lower.contains("heading")
            {
                fake::faker::lorem::en::Sentence(3..8).fake()
            } else if lower.contains("company")
                || lower.contains("organization")
                || lower.contains("org")
            {
                fake::faker::company::en::CompanyName().fake()
            } else if lower.contains("job") || lower.contains("position") {
                fake::faker::job::en::Title().fake()
            } else if lower == "first" || lower == "first_name" {
                fake::faker::name::en::FirstName().fake()
            } else if lower == "last"
                || lower == "last_name"
                || lower == "surname"
                || lower.contains("last")
            {
                fake::faker::name::en::LastName().fake()
            } else if lower.contains("username")
                || lower.contains("nick")
                || lower.contains("handle")
            {
                fake::faker::internet::en::Username().fake()
            } else {
                fake::faker::lorem::en::Word().fake()
            };
            serde_json::Value::String(val)
        }

        Kind::Bytes => serde_json::Value::String("c2FtcGxl".to_string()),

        Kind::Enum(enum_desc) => {
            let first = enum_desc.values().next();
            match first {
                Some(v) => serde_json::Value::String(v.name().to_string()),
                None => serde_json::Value::String("UNSPECIFIED".to_string()),
            }
        }

        Kind::Message(msg_desc) => match well_known_sample(msg_desc.full_name()) {
            Some(value) => value,
            None => generate_json_template(msg_desc),
        },
    }
}

pub(crate) fn well_known_sample(full_name: &str) -> Option<serde_json::Value> {
    Some(match full_name {
        "google.protobuf.Timestamp" => serde_json::json!("2024-06-15T10:30:00Z"),
        "google.protobuf.Duration" => serde_json::json!("30s"),
        "google.protobuf.FieldMask" => serde_json::json!("name"),
        "google.protobuf.Struct" => serde_json::json!({"key": "value"}),
        "google.protobuf.Value" => serde_json::json!("value"),
        "google.protobuf.StringValue" => serde_json::json!("value"),
        "google.protobuf.BoolValue" => serde_json::json!(true),
        "google.protobuf.Int32Value" | "google.protobuf.Int64Value" => serde_json::json!(1),
        "google.protobuf.DoubleValue" | "google.protobuf.FloatValue" => serde_json::json!(1.0),
        "google.protobuf.BytesValue" => serde_json::json!("c2FtcGxl"),
        "google.protobuf.Empty" => serde_json::json!({}),
        "google.protobuf.Any" => {
            serde_json::json!({"@type": "type.googleapis.com/replace.With.Your.Message"})
        }
        _ => return None,
    })
}

pub fn generate_json_template(desc: &prost_reflect::MessageDescriptor) -> serde_json::Value {
    let mut obj = serde_json::Map::new();

    for field in desc.fields() {
        let name = field.json_name().to_string();
        let fv = fake_value(&name, &field.kind());

        if field.is_list() {
            obj.insert(name, serde_json::Value::Array(vec![fv]));
        } else if field.is_map() {
            obj.insert(name, serde_json::Value::Object(serde_json::Map::new()));
        } else {
            obj.insert(name, fv);
        }
    }

    serde_json::Value::Object(obj)
}
