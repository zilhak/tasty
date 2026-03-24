# serde 1 + serde_json 1 + toml 0.8

Rust 직렬화/역직렬화 생태계. `serde`는 프레임워크, `serde_json`과 `toml`은 포맷 구현체다.

## Cargo.toml

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
```

## Serialize / Deserialize derive

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Config {
    font_size: f32,
    shell: String,
    color_scheme: String,
    keybindings: Vec<Keybinding>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Keybinding {
    key: String,
    action: String,
}
```

## #[serde] 속성 레퍼런스

### rename — 직렬화 키 이름 변경

```rust
#[derive(Serialize, Deserialize)]
struct Config {
    // JSON/TOML에서는 "fontSize"로, Rust에서는 font_size
    #[serde(rename = "fontSize")]
    font_size: f32,

    // 직렬화할 때만 이름 변경
    #[serde(rename(serialize = "lineHeight"))]
    line_height: f32,

    // 역직렬화할 때만 이름 변경
    #[serde(rename(deserialize = "bgColor"))]
    background_color: String,
}

// rename_all: 모든 필드에 적용
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]  // camelCase, snake_case, PascalCase, SCREAMING_SNAKE_CASE 등
struct ApiResponse {
    user_name: String,    // "userName"
    created_at: u64,      // "createdAt"
}
```

### default — 필드 기본값

```rust
#[derive(Serialize, Deserialize)]
struct Config {
    // 필드가 없으면 Default::default() 사용
    #[serde(default)]
    fullscreen: bool,

    // 커스텀 기본값 함수
    #[serde(default = "default_font_size")]
    font_size: f32,

    // 옵션 필드: None이 기본값
    #[serde(default)]
    theme: Option<String>,
}

fn default_font_size() -> f32 { 14.0 }

// 구조체 전체에 기본값 적용 (없는 필드 모두 Default 사용)
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct WindowConfig {
    width: u32,   // 기본값 0
    height: u32,  // 기본값 0
    title: String,
}
```

### skip — 필드 제외

```rust
#[derive(Serialize, Deserialize)]
struct Session {
    id: String,

    // 직렬화/역직렬화 모두 제외
    #[serde(skip)]
    internal_handle: Option<usize>,

    // 직렬화에서만 제외
    #[serde(skip_serializing)]
    write_only_token: String,

    // 역직렬화에서만 제외 (항상 계산)
    #[serde(skip_deserializing)]
    computed_hash: u64,

    // 조건부 skip: 값이 None일 때 직렬화 안 함
    #[serde(skip_serializing_if = "Option::is_none")]
    optional_field: Option<String>,

    // Vec가 비어있을 때 skip
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
}
```

### flatten — 중첩 구조체 평탄화

```rust
#[derive(Serialize, Deserialize)]
struct FontConfig {
    family: String,
    size: f32,
    weight: u16,
}

#[derive(Serialize, Deserialize)]
struct Config {
    shell: String,

    // FontConfig의 필드들이 Config와 같은 레벨로 평탄화됨
    // JSON: {"shell":"bash","family":"JetBrains Mono","size":14.0,"weight":400}
    #[serde(flatten)]
    font: FontConfig,
}

// HashMap 평탄화: 알려지지 않은 키를 수집
#[derive(Serialize, Deserialize)]
struct FlexConfig {
    known_field: String,
    #[serde(flatten)]
    extra: std::collections::HashMap<String, serde_json::Value>,
}
```

### with — 커스텀 직렬화 모듈

```rust
use serde::{Deserialize, Serialize};

mod hex_color {
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(color: &u32, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("#{:06X}", color))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
        let s: &str = serde::Deserialize::deserialize(d)?;
        u32::from_str_radix(s.trim_start_matches('#'), 16)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Serialize, Deserialize)]
struct Theme {
    #[serde(with = "hex_color")]
    foreground: u32,  // JSON: "#FFFFFF"

    #[serde(with = "hex_color")]
    background: u32,  // JSON: "#1E1E2E"
}
```

## serde_json

### json! 매크로

```rust
use serde_json::{json, Value};

let config = json!({
    "font_size": 14,
    "shell": "/bin/bash",
    "colors": {
        "foreground": "#FFFFFF",
        "background": "#1E1E2E"
    },
    "keybindings": [
        {"key": "Ctrl+T", "action": "new_tab"},
        {"key": "Ctrl+W", "action": "close_tab"}
    ]
});

// 값 접근
let font_size = config["font_size"].as_f64().unwrap_or(14.0);
let shell = config["shell"].as_str().unwrap_or("bash");
let fg = &config["colors"]["foreground"];
```

### to_string / from_str

```rust
use serde::{Deserialize, Serialize};
use serde_json;

#[derive(Serialize, Deserialize, Debug)]
struct Config {
    font_size: f32,
    shell: String,
}

// 직렬화
let config = Config {
    font_size: 14.0,
    shell: "/bin/bash".to_string(),
};

let json_str = serde_json::to_string(&config).unwrap();
// 결과: {"font_size":14.0,"shell":"/bin/bash"}

let json_pretty = serde_json::to_string_pretty(&config).unwrap();
// 결과: 들여쓰기된 JSON

// 역직렬화
let config2: Config = serde_json::from_str(&json_str).unwrap();

// 파일 읽기/쓰기
let file = std::fs::File::open("config.json").unwrap();
let config3: Config = serde_json::from_reader(file).unwrap();

let file = std::fs::File::create("config.json").unwrap();
serde_json::to_writer_pretty(file, &config).unwrap();
```

### 동적 Value 조작

```rust
use serde_json::{Map, Value};

// Value 직접 구성
let mut obj = Map::new();
obj.insert("key".to_string(), Value::String("value".to_string()));
obj.insert("num".to_string(), Value::Number(42.into()));
let v = Value::Object(obj);

// 타입 체크 및 변환
match &v["key"] {
    Value::String(s) => println!("문자열: {s}"),
    Value::Number(n) => println!("숫자: {n}"),
    Value::Bool(b) => println!("불리언: {b}"),
    Value::Array(arr) => println!("배열 길이: {}", arr.len()),
    Value::Object(map) => println!("객체 키: {}", map.len()),
    Value::Null => println!("null"),
}

// 포인터로 중첩 접근 (JSON Pointer 문법)
let deep = json!({"a": {"b": {"c": 42}}});
let val = deep.pointer("/a/b/c").and_then(|v| v.as_i64());
assert_eq!(val, Some(42));

// 머지
fn merge(base: &mut Value, patch: Value) {
    if let (Value::Object(base_map), Value::Object(patch_map)) = (base, patch) {
        for (k, v) in patch_map {
            merge(base_map.entry(k).or_insert(Value::Null), v);
        }
    }
}
```

## toml 0.8

### from_str / to_string

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
struct TastyConfig {
    #[serde(default)]
    general: GeneralConfig,
    #[serde(default)]
    font: FontConfig,
    #[serde(default)]
    colors: ColorsConfig,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct GeneralConfig {
    #[serde(default = "default_shell")]
    shell: String,
    #[serde(default)]
    fullscreen: bool,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct FontConfig {
    #[serde(default = "default_family")]
    family: String,
    #[serde(default = "default_size")]
    size: f32,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct ColorsConfig {
    #[serde(default = "default_fg")]
    foreground: String,
    #[serde(default = "default_bg")]
    background: String,
}

fn default_shell() -> String { "bash".to_string() }
fn default_family() -> String { "JetBrains Mono".to_string() }
fn default_size() -> f32 { 14.0 }
fn default_fg() -> String { "#FFFFFF".to_string() }
fn default_bg() -> String { "#1E1E2E".to_string() }

// 역직렬화 (파일 파싱)
fn load_config(path: &str) -> Result<TastyConfig, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let config: TastyConfig = toml::from_str(&content)?;
    Ok(config)
}

// 직렬화 (파일 저장)
fn save_config(config: &TastyConfig, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let content = toml::to_string_pretty(config)?;
    std::fs::write(path, content)?;
    Ok(())
}
```

### TOML 설정 파일 예시

위 구조체에 대응하는 `~/.tasty/config.toml`:

```toml
[general]
shell = "/bin/zsh"
fullscreen = false

[font]
family = "JetBrains Mono"
size = 14.0

[colors]
foreground = "#CDD6F4"
background = "#1E1E2E"
```

### toml::Value 동적 파싱

```rust
use toml::Value;

let content = std::fs::read_to_string("config.toml").unwrap();
let parsed: Value = content.parse().unwrap();

// 테이블 접근
if let Some(font) = parsed.get("font") {
    let size = font.get("size")
        .and_then(|v| v.as_float())
        .unwrap_or(14.0);
    println!("폰트 크기: {size}");
}

// 배열 접근
if let Some(Value::Array(keys)) = parsed.get("keybindings") {
    for key in keys {
        println!("{:?}", key);
    }
}
```

## 에러 처리 패턴

```rust
use std::fmt;

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Toml(toml::de::Error),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "파일 오류: {e}"),
            ConfigError::Json(e) => write!(f, "JSON 파싱 오류: {e}"),
            ConfigError::Toml(e) => write!(f, "TOML 파싱 오류: {e}"),
        }
    }
}

impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self { ConfigError::Io(e) }
}
impl From<serde_json::Error> for ConfigError {
    fn from(e: serde_json::Error) -> Self { ConfigError::Json(e) }
}
impl From<toml::de::Error> for ConfigError {
    fn from(e: toml::de::Error) -> Self { ConfigError::Toml(e) }
}

// 사용
fn load(path: &str) -> Result<TastyConfig, ConfigError> {
    let s = std::fs::read_to_string(path)?;
    let c: TastyConfig = toml::from_str(&s)?;
    Ok(c)
}
```

## 자주 쓰는 serde 조합

| 용도 | 방법 |
|------|------|
| 필드 없으면 기본값 | `#[serde(default)]` |
| None이면 필드 생략 | `#[serde(skip_serializing_if = "Option::is_none")]` |
| 빈 Vec 생략 | `#[serde(skip_serializing_if = "Vec::is_empty")]` |
| 모든 필드 camelCase | `#[serde(rename_all = "camelCase")]` |
| 중첩 구조 평탄화 | `#[serde(flatten)]` |
| 알 수 없는 필드 수집 | `#[serde(flatten)] extra: HashMap<String, Value>` |
| 알 수 없는 필드 무시 | `#[serde(deny_unknown_fields)]` 반대: 기본 동작이 무시 |
