// ---------------------------------------------------------------------------
// init.ts —— peri-sandbox init（初始化 Daytona 连接）
// ---------------------------------------------------------------------------
import { input, password, confirm } from "@inquirer/prompts";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { loadDaytonaConfig } from "../daytona-helpers";

function configDir(): string {
    return path.join(os.homedir(), ".peri-sandbox");
}

function configPath(): string {
    return path.join(configDir(), "config.json");
}

export async function runInit(): Promise<void> {
    console.log("\n初始化 Daytona 连接\n");

    const { apiUrl: defaultUrl } = loadDaytonaConfig();

    const apiKey = await password({
        message: "Daytona API Key",
        mask: "*",
    });
    const apiUrl = await input({
        message: "Daytona API URL",
        default: defaultUrl || "https://app.daytona.io/api",
    });

    if (!apiKey) {
        console.error("\nAPI Key 不能为空");
        process.exit(1);
    }

    console.log("\n即将保存:\n");
    console.log(`  API Key    ****${apiKey.slice(-4)}`);
    console.log(`  API URL    ${apiUrl}`);
    console.log(`  保存位置    ${configPath()}`);

    const ok = await confirm({ message: "\n确认保存?", default: true });
    if (!ok) {
        console.log("已取消。");
        return;
    }

    fs.mkdirSync(configDir(), { recursive: true });
    fs.writeFileSync(configPath(), JSON.stringify({ apiKey, apiUrl }, null, 2), "utf-8");
    console.log(`\n已保存到 ${configPath()}`);
    console.log(`现在可以运行: peri-sandbox create`);
}
