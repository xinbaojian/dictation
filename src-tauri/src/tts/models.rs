//! 语音包目录：设置页「语音包」区的数据源，常量表 + 纯函数构建引擎配置。
//!
//! 归档结构来自官方文档（2026-11 核对）；`required` 清单在安装后校验，
//! 也作为「已安装」的判定依据。文件名若与上游发布不符，会在下载完成
//! 校验 / 引擎创建时暴露，届时只需改这张表。

use crate::error::{AppError, AppResult};
use sherpa_onnx::{
    OfflineTtsConfig, OfflineTtsKokoroModelConfig, OfflineTtsMatchaModelConfig,
    OfflineTtsModelConfig, OfflineTtsVitsModelConfig,
};
use std::path::{Path, PathBuf};

/// 一个语音包的下载源：`.tar.bz2` 归档（解压后取顶层目录内容）或裸文件
pub enum Source {
    TarBz2 { url: &'static str },
    File { url: &'static str, name: &'static str },
}

/// 引擎家族（决定 OfflineTtsConfig 怎么填）
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Kokoro,
    Matcha,
    Vits,
}

pub struct ModelSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// 解压后总大小（MB，展示用约数）
    pub size_mb: u64,
    /// 许可证提示（matcha 训练数据非商用等）
    pub license_note: Option<&'static str>,
    pub sources: &'static [Source],
    /// 模型目录下必须存在的文件 / 目录（目录以 `/` 结尾）
    pub required: &'static [&'static str],
    /// 主 onnx 文件名（kokoro int8 是 model.int8.onnx，其余是 model.onnx）
    pub onnx: &'static str,
    /// 多音色模型的默认说话人。kokoro v1.1 的中文女声是 sid 3-57（zf_001…zf_099），
    /// 与 v1.0 的编号表完全不同，别照抄旧表
    pub sid: i32,
    pub kind: Kind,
}

pub static MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "kokoro-int8-v1.1",
        name: "Kokoro 中英语（推荐）",
        description: "中英文 · 103 种音色 · 音质与体积的平衡之选（int8 量化）",
        size_mb: 120,
        license_note: None,
        sources: &[Source::TarBz2 {
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/kokoro-int8-multi-lang-v1_1.tar.bz2",
        }],
        // 归档实测（2026-11-08）：model.int8.onnx / voices.bin / tokens.txt /
        // lexicon-{zh,us-en,gb-en}.txt / dict/ / espeak-ng-data/ / *-zh.fst
        required: &[
            "model.int8.onnx",
            "voices.bin",
            "tokens.txt",
            "lexicon-zh.txt",
            "lexicon-us-en.txt",
            "dict/",
            "espeak-ng-data/",
            "phone-zh.fst",
            "number-zh.fst",
            "date-zh.fst",
        ],
        onnx: "model.int8.onnx",
        sid: 3,
        kind: Kind::Kokoro,
    },
    ModelSpec {
        id: "matcha-zh-baker",
        name: "Matcha 中文女声",
        description: "中文女声 · 合成速度最快 · 适合低配电脑（含 51MB 声码器）",
        size_mb: 130,
        license_note: Some("训练数据（Data-Baker）仅限非商用使用"),
        sources: &[
            Source::TarBz2 {
                url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/matcha-icefall-zh-baker.tar.bz2",
            },
            Source::File {
                url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/vocoder-models/vocos-22khz-univ.onnx",
                name: "vocos-22khz-univ.onnx",
            },
        ],
        required: &[
            "model-steps-3.onnx",
            "vocos-22khz-univ.onnx",
            "lexicon.txt",
            "tokens.txt",
            "dict/",
            "phone.fst",
            "date.fst",
            "number.fst",
        ],
        onnx: "model-steps-3.onnx",
        sid: 0,
        kind: Kind::Matcha,
    },
    ModelSpec {
        id: "melo-zh-en",
        name: "Melo 中英语混读",
        description: "中英文 · 单音色 · 单词里夹英文时读得自然",
        size_mb: 170,
        license_note: None,
        sources: &[Source::TarBz2 {
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/vits-melo-tts-zh_en.tar.bz2",
        }],
        required: &[
            "model.onnx",
            "lexicon.txt",
            "tokens.txt",
            "dict/",
            "date.fst",
            "number.fst",
        ],
        onnx: "model.onnx",
        sid: 0,
        kind: Kind::Vits,
    },
    ModelSpec {
        id: "kokoro-v1.1",
        name: "Kokoro 中英语（完整版）",
        description: "中英文 · 103 种音色 · 未量化，音质上限档",
        size_mb: 330,
        license_note: None,
        sources: &[Source::TarBz2 {
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/kokoro-multi-lang-v1_1.tar.bz2",
        }],
        required: &[
            "model.onnx",
            "voices.bin",
            "tokens.txt",
            "lexicon-zh.txt",
            "lexicon-us-en.txt",
            "dict/",
            "espeak-ng-data/",
            "phone-zh.fst",
            "number-zh.fst",
            "date-zh.fst",
        ],
        onnx: "model.onnx",
        sid: 3,
        kind: Kind::Kokoro,
    },
];

pub fn find(id: &str) -> Option<&'static ModelSpec> {
    MODELS.iter().find(|m| m.id == id)
}

/// 数据目录下的语音包根目录：`<data_dir>/tts-model/`
pub fn model_root(data_dir: &Path) -> PathBuf {
    data_dir.join("tts-model")
}

/// 某个语音包解压后的目录
pub fn model_dir(data_dir: &Path, id: &str) -> PathBuf {
    model_root(data_dir).join(id)
}

/// 「已安装」= 必需文件全部在位（目录项以 `/` 结尾表示）
pub fn is_installed(spec: &ModelSpec, data_dir: &Path) -> bool {
    let dir = model_dir(data_dir, spec.id);
    spec.required.iter().all(|req| {
        let path = dir.join(req.trim_end_matches('/'));
        if req.ends_with('/') {
            path.is_dir()
        } else {
            path.is_file()
        }
    })
}

/// 安装完成后校验必需文件，缺哪个报哪个（中文提示直接可展示）
pub fn verify(spec: &ModelSpec, dir: &Path) -> AppResult<()> {
    for req in spec.required {
        let path = dir.join(req.trim_end_matches('/'));
        let ok = if req.ends_with('/') { path.is_dir() } else { path.is_file() };
        if !ok {
            return Err(AppError::msg(format!("语音包文件缺失：{req}（可能下载不完整，请删除后重新下载）")));
        }
    }
    Ok(())
}

/// 构建引擎配置。路径全部取绝对路径；lexicon / rule_fsts 是逗号分隔的多路径。
pub fn build_config(spec: &ModelSpec, dir: &Path) -> OfflineTtsConfig {
    let p = |name: &str| Some(dir.join(name).to_string_lossy().into_owned());
    let joined = |names: &[&str]| names.iter().map(|n| dir.join(n).to_string_lossy().into_owned()).collect::<Vec<_>>().join(",");
    let mut config = OfflineTtsConfig {
        model: OfflineTtsModelConfig {
            num_threads: 2,
            debug: false,
            ..Default::default()
        },
        ..Default::default()
    };
    match spec.kind {
        Kind::Kokoro => {
            // 照官方 v1.1 Rust 示例：lexicon（us-en,zh）+ espeak-ng-data，
            // 不传 dict_dir / rule_fsts（运行时自动发现归档里的 dict/）
            config.model.kokoro = OfflineTtsKokoroModelConfig {
                model: p(spec.onnx),
                voices: p("voices.bin"),
                tokens: p("tokens.txt"),
                data_dir: p("espeak-ng-data"),
                // 官方示例顺序：us-en 在前 zh 在后
                lexicon: Some(joined(&["lexicon-us-en.txt", "lexicon-zh.txt"])),
                length_scale: 1.0,
                ..Default::default()
            };
        }
        Kind::Matcha => {
            config.model.matcha = OfflineTtsMatchaModelConfig {
                acoustic_model: p("model-steps-3.onnx"),
                vocoder: p("vocos-22khz-univ.onnx"),
                lexicon: p("lexicon.txt"),
                tokens: p("tokens.txt"),
                dict_dir: p("dict"),
                noise_scale: 0.667,
                length_scale: 1.0,
                ..Default::default()
            };
            config.rule_fsts = Some(joined(&["phone.fst", "date.fst", "number.fst"]));
        }
        Kind::Vits => {
            config.model.vits = OfflineTtsVitsModelConfig {
                model: p("model.onnx"),
                lexicon: p("lexicon.txt"),
                tokens: p("tokens.txt"),
                dict_dir: p("dict"),
                ..Default::default()
            };
            config.rule_fsts = Some(joined(&["date.fst", "number.fst"]));
        }
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 下载源只允许这两个官方前缀（防目录表被误改成任意源）
    const TTS_MODELS_BASE: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models";
    const VOCODER_MODELS_BASE: &str =
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/vocoder-models";

    /// 独立重算配置引用的文件清单（与 build_config 的字面量双份校验）
    fn config_paths(spec: &ModelSpec, dir: &Path) -> Vec<PathBuf> {
        let kokoro: &[&str] = &[
            spec.onnx,
            "voices.bin",
            "tokens.txt",
            "lexicon-us-en.txt",
            "lexicon-zh.txt",
            "espeak-ng-data",
        ];
        let names: &[&[&str]] = match spec.kind {
            Kind::Kokoro => &[kokoro],
            Kind::Matcha => &[
                &["model-steps-3.onnx", "vocos-22khz-univ.onnx", "lexicon.txt", "tokens.txt", "dict"],
                &["phone.fst", "date.fst", "number.fst"],
            ],
            Kind::Vits => &[
                &["model.onnx", "lexicon.txt", "tokens.txt", "dict"],
                &["date.fst", "number.fst"],
            ],
        };
        names.iter().flat_map(|g| g.iter()).map(|n| dir.join(n)).collect()
    }

    /// 每个测试用独立临时目录：并行测试共用目录会互相 remove
    fn tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dictation-tts-models-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<_> = MODELS.iter().map(|m| m.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "语音包 id 必须唯一");
    }

    /// 下载地址必须是官方 GitHub Releases，防止目录表被误改成任意源
    #[test]
    fn urls_are_official_github_releases() {
        for m in MODELS {
            assert!(!m.sources.is_empty(), "{} 至少要有一个下载源", m.id);
            for s in m.sources {
                let url = match s {
                    Source::TarBz2 { url } | Source::File { url, .. } => *url,
                };
                let ok = url.starts_with(TTS_MODELS_BASE) || url.starts_with(VOCODER_MODELS_BASE);
                assert!(ok, "{} 的下载源不是官方 GitHub Releases：{url}", m.id);
                assert!(url.starts_with("https://"), "必须是 HTTPS");
            }
        }
    }

    /// 引擎配置引用的每个文件都必须在 required 清单里：漏写 required 会让
    /// 「已安装」判定虚高，换机器拷数据目录时静默缺文件。
    #[test]
    fn config_files_covered_by_required() {
        for m in MODELS {
            for path in config_paths(m, Path::new("/x")) {
                let name = path.file_name().unwrap().to_str().unwrap();
                assert!(
                    m.required.iter().any(|r| r.trim_end_matches('/') == name),
                    "{} 的配置引用了 {}，但 required 清单里没有",
                    m.id,
                    name
                );
            }
        }
    }

    #[test]
    fn is_installed_requires_every_file() {
        let dir = tmp("installed");
        let spec = find("matcha-zh-baker").unwrap();
        assert!(!is_installed(spec, &dir));
        let model_dir = dir.join("tts-model").join(spec.id);
        std::fs::create_dir_all(&model_dir).unwrap();
        for req in spec.required {
            let path = model_dir.join(req.trim_end_matches('/'));
            if req.ends_with('/') {
                std::fs::create_dir_all(&path).unwrap();
            } else {
                std::fs::write(&path, b"x").unwrap();
            }
        }
        assert!(is_installed(spec, &dir));
        // 抠掉一个文件就不再算已安装
        std::fs::remove_file(model_dir.join("tokens.txt")).unwrap();
        assert!(!is_installed(spec, &dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_names_the_missing_file() {
        let dir = tmp("verify");
        let spec = find("melo-zh-en").unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let err = verify(spec, &dir).unwrap_err().to_string();
        assert!(err.contains("model.onnx"), "报错要能定位缺的文件：{err}");
    }

    /// 配置里的路径全部落在模型目录内（绝对路径、以目录开头）
    #[test]
    fn build_config_paths_are_inside_model_dir() {
        let dir = tmp("paths");
        let model_dir = dir.join("tts-model/matcha-zh-baker");
        std::fs::create_dir_all(&model_dir).unwrap();
        let spec = find("matcha-zh-baker").unwrap();
        let config = build_config(spec, &model_dir);
        assert!(config.model.matcha.acoustic_model.unwrap().starts_with(
            model_dir.to_str().unwrap()
        ));
        let rule = config.rule_fsts.unwrap();
        assert!(rule.contains("phone.fst") && rule.contains("number.fst"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
