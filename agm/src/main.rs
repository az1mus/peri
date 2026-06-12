use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "agm", about = "Agent Package Manager")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// 项目目录（默认当前目录）
    #[arg(short = 'C', long, default_value = ".", global = true)]
    dir: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// 生成 agm.json 模板
    Init,

    /// 安装所有依赖到目标工具
    Install {
        /// 目标工具 (claude, codex, copilot)
        #[arg(long, default_value = "claude")]
        tool: String,

        /// 从 git URL 直接安装（如 https://github.com/user/repo）
        #[arg(long)]
        git: Option<String>,
    },

    /// 卸载一个包
    Uninstall {
        /// 包名
        package: String,
        /// 目标工具
        #[arg(long, default_value = "claude")]
        tool: String,
    },

    /// 检查可升级的包
    Update,

    /// 列出所有依赖
    List,

    /// 发布包到 registry
    Publish {
        /// Registry URL
        #[arg(long)]
        registry: Option<String>,
    },

    /// 清理 store 中的孤儿包
    Gc,

    /// 更新 agm 自身
    SelfUpdate {
        /// 强制重新安装（即使已是最新）
        #[arg(long)]
        force: bool,
    },
}

fn main() {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let dir = cli.dir.map(PathBuf::from);

    let result = match cli.command {
        Commands::Init => agm::commands::init::execute(dir),
        Commands::Install { tool, git } => {
            agm::commands::install::execute(&tool, git.as_deref(), dir)
        }
        Commands::Uninstall { package, tool } => {
            agm::commands::uninstall::execute(&package, &tool, dir)
        }
        Commands::Update => agm::commands::update::execute(dir),
        Commands::List => agm::commands::list::execute(dir),
        Commands::Publish { registry } => agm::commands::publish::execute(registry, dir),
        Commands::Gc => agm::commands::gc::execute(),
        Commands::SelfUpdate { force } => agm::commands::self_update::execute(force),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
