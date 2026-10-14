//! 类型绑定导出。
//!
//! 各类型标注了 `#[ts(export)]`,ts-rs 会为每个类型生成 `export_bindings_*` 测试,
//! 运行它们即把定义写入 src/generated/*.ts(用 `npm run types:gen`)。
//! 本测试只确保导出目录存在,给构建流程一个明确的失败点。
//!
//! 生成的类型是后端结构的权威定义;前端 src/types.ts 以它为准手工对齐。

#[test]
fn export_bindings_generated_dir_created() {
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("src")
        .join("generated");
    std::fs::create_dir_all(&out).expect("create generated dir");
    assert!(out.exists(), "generated 目录应存在");
}
