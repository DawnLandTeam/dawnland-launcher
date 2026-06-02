const fs = require('fs');
const path = require('path');

const zhFile = path.resolve('src/locales/zh-CN.json');
const enFile = path.resolve('src/locales/en.json');

const zhData = JSON.parse(fs.readFileSync(zhFile, 'utf8'));
const enData = JSON.parse(fs.readFileSync(enFile, 'utf8'));

// Update servers namespace
zhData.servers.actions.installInstance = "安装实例";
zhData.servers.actions.details = "详情";

enData.servers.actions.installInstance = "Install Instance";
enData.servers.actions.details = "Details";

zhData.servers.publishDialog.steps.details = "详情与联系方式 (Details)";
enData.servers.publishDialog.steps.details = "Details & Contact";

zhData.servers.publishDialog.tags = "标签/徽标 (Tags) - 逗号分隔";
zhData.servers.publishDialog.tagsPlaceholder = "例如: 生存, 魔法, 科技";
zhData.servers.publishDialog.description = "服务器详情介绍 (Description - Markdown)";
zhData.servers.publishDialog.descPlaceholder = "支持 Markdown 格式。详细介绍您的服务器...";
zhData.servers.publishDialog.contactGroup = "交流群 (Community Group)";
zhData.servers.publishDialog.contactGroupPlaceholder = "例如: QQ群 12345678";
zhData.servers.publishDialog.contactOwner = "服主联系方式 (Owner Contact)";
zhData.servers.publishDialog.contactOwnerPlaceholder = "例如: admin@example.com";

enData.servers.publishDialog.tags = "Tags - Comma separated";
enData.servers.publishDialog.tagsPlaceholder = "e.g. survival, magic, tech";
enData.servers.publishDialog.description = "Server Description (Markdown)";
enData.servers.publishDialog.descPlaceholder = "Markdown supported. Describe your server in detail...";
enData.servers.publishDialog.contactGroup = "Community Group";
enData.servers.publishDialog.contactGroupPlaceholder = "e.g. Discord link";
enData.servers.publishDialog.contactOwner = "Owner Contact";
enData.servers.publishDialog.contactOwnerPlaceholder = "e.g. admin@example.com";

zhData.servers.messages.authlibRequired = "此服务器需要 Authlib 账号进行验证（API：{api}）。请先在账号管理中添加对应的 Authlib 账号。";
enData.servers.messages.authlibRequired = "This server requires Authlib verification (API: {api}). Please add the corresponding Authlib account in Account Settings first.";

// Update home namespace
zhData.home.noInstanceFound = "没有找到匹配的实例 (No installed instance found for server {name}). 请先安装它。";
zhData.home.noAccountFound = "没有找到可用账号 (No account found). 请在设置中添加账号。";
zhData.home.launchFailed = "启动失败 (Failed to launch): {error}";
zhData.home.stopGame = "停止运行";
zhData.home.stopGameConfirmTitle = "强制停止游戏";
zhData.home.stopGameConfirm = "你确定要强制结束正在运行的游戏进程吗？";
zhData.home.stopGameFailed = "停止游戏失败: {error}";
zhData.home.addAccount = "添加新账号";
zhData.home.repairing = "正在校验与修复 Mod...";
zhData.home.repairFound = "检测到 {total} 个文件缺失或损坏，正在自动重新下载。";
zhData.home.repairProgress = "{completed} / {total} 文件";

enData.home.noInstanceFound = "No installed instance found for server {name}. Please install it first.";
enData.home.noAccountFound = "No account found. Please add an account in Settings.";
enData.home.launchFailed = "Failed to launch: {error}";
enData.home.stopGame = "Stop Game";
enData.home.stopGameConfirmTitle = "Force Stop Game";
enData.home.stopGameConfirm = "Are you sure you want to force kill the running game process?";
enData.home.stopGameFailed = "Failed to stop game: {error}";
enData.home.addAccount = "Add New Account";
enData.home.repairing = "Verifying & Repairing Mods...";
enData.home.repairFound = "Detected {total} missing or corrupted files, re-downloading automatically.";
enData.home.repairProgress = "{completed} / {total} files";

fs.writeFileSync(zhFile, JSON.stringify(zhData, null, 2) + '\n');
fs.writeFileSync(enFile, JSON.stringify(enData, null, 2) + '\n');

console.log('Locales updated successfully!');
