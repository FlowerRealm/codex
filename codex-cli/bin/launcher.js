import { spawn } from "node:child_process";
import { existsSync } from "fs";
import { createRequire } from "node:module";
import path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const require = createRequire(import.meta.url);

const CLI_NPM_NAME = "@flowerrealm/realmx";
const CLI_NAME_ENV_VAR = "CODEX_CLI_NAME";
const LEGACY_BINARY_DIR = "codex";
const LEGACY_BINARY_NAME = process.platform === "win32" ? "codex.exe" : "codex";

const PLATFORM_PACKAGE_BY_TARGET = {
  "x86_64-unknown-linux-musl": `${CLI_NPM_NAME}-linux-x64`,
  "aarch64-unknown-linux-musl": `${CLI_NPM_NAME}-linux-arm64`,
  "x86_64-apple-darwin": `${CLI_NPM_NAME}-darwin-x64`,
  "aarch64-apple-darwin": `${CLI_NPM_NAME}-darwin-arm64`,
  "x86_64-pc-windows-msvc": `${CLI_NPM_NAME}-win32-x64`,
  "aarch64-pc-windows-msvc": `${CLI_NPM_NAME}-win32-arm64`,
};

function resolveTargetTriple() {
  const { platform, arch } = process;

  switch (platform) {
    case "linux":
    case "android":
      switch (arch) {
        case "x64":
          return "x86_64-unknown-linux-musl";
        case "arm64":
          return "aarch64-unknown-linux-musl";
        default:
          return null;
      }
    case "darwin":
      switch (arch) {
        case "x64":
          return "x86_64-apple-darwin";
        case "arm64":
          return "aarch64-apple-darwin";
        default:
          return null;
      }
    case "win32":
      switch (arch) {
        case "x64":
          return "x86_64-pc-windows-msvc";
        case "arm64":
          return "aarch64-pc-windows-msvc";
        default:
          return null;
      }
    default:
      return null;
  }
}

function getUpdatedPath(newDirs) {
  const pathSep = process.platform === "win32" ? ";" : ":";
  const existingPath = process.env.PATH || "";
  return [...newDirs, ...existingPath.split(pathSep).filter(Boolean)].join(pathSep);
}

function detectPackageManager() {
  const userAgent = process.env.npm_config_user_agent || "";
  if (/\bbun\//.test(userAgent)) {
    return "bun";
  }

  const execPath = process.env.npm_execpath || "";
  if (execPath.includes("bun")) {
    return "bun";
  }

  if (
    __dirname.includes(".bun/install/global") ||
    __dirname.includes(".bun\\install\\global")
  ) {
    return "bun";
  }

  return userAgent ? "npm" : null;
}

export function runCli(cliName) {
  const targetTriple = resolveTargetTriple();
  if (!targetTriple) {
    throw new Error(`Unsupported platform: ${process.platform} (${process.arch})`);
  }

  const platformPackage = PLATFORM_PACKAGE_BY_TARGET[targetTriple];
  if (!platformPackage) {
    throw new Error(`Unsupported target triple: ${targetTriple}`);
  }

  const localVendorRoot = path.join(__dirname, "..", "vendor");
  const localBinaryPath = path.join(
    localVendorRoot,
    targetTriple,
    LEGACY_BINARY_DIR,
    LEGACY_BINARY_NAME,
  );

  let vendorRoot;
  try {
    const packageJsonPath = require.resolve(`${platformPackage}/package.json`);
    vendorRoot = path.join(path.dirname(packageJsonPath), "vendor");
  } catch {
    if (existsSync(localBinaryPath)) {
      vendorRoot = localVendorRoot;
    } else {
      const packageManager = detectPackageManager();
      const updateCommand =
        packageManager === "bun"
          ? `bun install -g ${CLI_NPM_NAME}@latest`
          : `npm install -g ${CLI_NPM_NAME}@latest`;
      throw new Error(
        `Missing optional dependency ${platformPackage}. Reinstall Realmx: ${updateCommand}`,
      );
    }
  }

  if (!vendorRoot) {
    const packageManager = detectPackageManager();
    const updateCommand =
      packageManager === "bun"
        ? `bun install -g ${CLI_NPM_NAME}@latest`
        : `npm install -g ${CLI_NPM_NAME}@latest`;
    throw new Error(
      `Missing optional dependency ${platformPackage}. Reinstall Realmx: ${updateCommand}`,
    );
  }

  const archRoot = path.join(vendorRoot, targetTriple);
  const binaryPath = path.join(archRoot, LEGACY_BINARY_DIR, LEGACY_BINARY_NAME);

  const additionalDirs = [];
  const pathDir = path.join(archRoot, "path");
  if (existsSync(pathDir)) {
    additionalDirs.push(pathDir);
  }
  const updatedPath = getUpdatedPath(additionalDirs);

  const env = {
    ...process.env,
    PATH: updatedPath,
    [CLI_NAME_ENV_VAR]: cliName,
  };
  const packageManagerEnvVar =
    detectPackageManager() === "bun"
      ? "CODEX_MANAGED_BY_BUN"
      : "CODEX_MANAGED_BY_NPM";
  env[packageManagerEnvVar] = "1";

  const child = spawn(binaryPath, process.argv.slice(2), {
    stdio: "inherit",
    env,
    argv0: cliName,
  });

  child.on("error", (err) => {
    console.error(err);
    process.exit(1);
  });

  const forwardSignal = (signal) => {
    if (child.killed) {
      return;
    }
    try {
      child.kill(signal);
    } catch {
      // ignore
    }
  };

  ["SIGINT", "SIGTERM", "SIGHUP"].forEach((sig) => {
    process.on(sig, () => forwardSignal(sig));
  });

  child.on("exit", (code, signal) => {
    if (signal) {
      process.kill(process.pid, signal);
      return;
    }
    process.exit(code ?? 1);
  });
}
