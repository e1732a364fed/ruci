/*!
 * chain 包定义了 chain 配置格式. 静态配置既可以使用 lua 也可以使用 toml
 */

/// defines config formats for chain mode
pub mod config;

/// actual runnable engine for chain mode
pub mod engine;
