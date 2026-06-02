const fs = require('fs');
const path = require('path');

const zhFile = path.resolve('src/locales/zh-CN.json');
const enFile = path.resolve('src/locales/en.json');

const zhData = JSON.parse(fs.readFileSync(zhFile, 'utf8'));
const enData = JSON.parse(fs.readFileSync(enFile, 'utf8'));

// ModpackInstallView
if (!zhData.modpacks) zhData.modpacks = {};
if (!enData.modpacks) enData.modpacks = {};

zhData.install.modularTitle = "模块化实例装配台";
zhData.install.modularDesc = "灵活组装你的 Mod 加载器和基础组件。系统会自动校验不兼容的组合。";
zhData.install.uninstalled = "不安装"; // Could also be used for 不安裝
zhData.install.selectFabric = "请选择 Fabric 版本";
zhData.install.selectForge = "请选择 Forge 版本";
zhData.install.selectNeoForge = "请选择 NeoForge 版本";
zhData.install.selectVersion = "请选择版本";
zhData.install.loading = "加载中...";
zhData.install.noVersionNeeded = "该模组暂无需配置具体版本，将在生成实例后自动下载。";

enData.install.modularTitle = "Modular Instance Assembler";
enData.install.modularDesc = "Flexibly assemble your Mod loader and base components. The system automatically verifies incompatible combinations.";
enData.install.uninstalled = "Not installed";
enData.install.selectFabric = "Select Fabric version";
enData.install.selectForge = "Select Forge version";
enData.install.selectNeoForge = "Select NeoForge version";
enData.install.selectVersion = "Select version";
enData.install.loading = "Loading...";
enData.install.noVersionNeeded = "No specific version config needed, it will be automatically downloaded.";

zhData.modpacks.searchPlaceholder = "搜索整合包名称...";
zhData.modpacks.searchOnline = "在线搜索";
zhData.modpacks.uploadLocal = "本地上传";
zhData.modpacks.searchBtn = "搜索";
zhData.modpacks.searchHint = "输入关键词开始搜索整合包";
zhData.modpacks.instanceNamePrefix = "将要创建的实例名称";
zhData.modpacks.packVersion = "整合包版本";
zhData.modpacks.gameVersion = "游戏版本";
zhData.modpacks.loader = "加载器";
zhData.modpacks.publishDate = "发布日期";
zhData.modpacks.actions = "操作";
zhData.modpacks.installBtn = "安装";

enData.modpacks.searchPlaceholder = "Search modpack name...";
enData.modpacks.searchOnline = "Online Search";
enData.modpacks.uploadLocal = "Local Upload";
enData.modpacks.searchBtn = "Search";
enData.modpacks.searchHint = "Enter keywords to search for modpacks";
enData.modpacks.instanceNamePrefix = "Instance name to create";
enData.modpacks.packVersion = "Modpack Version";
enData.modpacks.gameVersion = "Game Version";
enData.modpacks.loader = "Loader";
enData.modpacks.publishDate = "Publish Date";
enData.modpacks.actions = "Actions";
enData.modpacks.installBtn = "Install";

if (!zhData.servers.details) zhData.servers.details = {};
if (!enData.servers.details) enData.servers.details = {};

zhData.servers.details.vanilla = "原版 (Vanilla)";
zhData.servers.details.serverAddress = "连接地址 (Server Address)";
zhData.servers.details.auth = "MOTD & 验证 (Auth)";
zhData.servers.details.authMicrosoft = "验证方式: 正版验证 (Microsoft)";
zhData.servers.details.contact = "联系方式 (Contact)";
zhData.servers.details.communityGroup = "交流群 (Community Group)";
zhData.servers.details.ownerContact = "服主联系方式 (Owner Contact)";
zhData.servers.details.description = "服务器详情 (Description)";
zhData.servers.details.modded = "模组 (Modded)";
zhData.servers.details.noDescription = "暂无介绍 (No description provided)";
zhData.servers.details.authAuthlib = "验证方式: 外置登录 (Authlib)";

enData.servers.details.vanilla = "Vanilla";
enData.servers.details.serverAddress = "Server Address";
enData.servers.details.auth = "MOTD & Auth";
enData.servers.details.authMicrosoft = "Auth: Microsoft";
enData.servers.details.contact = "Contact Info";
enData.servers.details.communityGroup = "Community Group";
enData.servers.details.ownerContact = "Owner Contact";
enData.servers.details.description = "Server Description";
enData.servers.details.modded = "Modded";
enData.servers.details.noDescription = "No description provided";
enData.servers.details.authAuthlib = "Auth: Authlib";

zhData.accounts.noAuthlibServers = "暂无已添加的认证服务器，\n请先前往设置页面进行添加。";
zhData.accounts.authlibTab = "外置登录";
zhData.accounts.goToSettings = "前往设置管理";

enData.accounts.noAuthlibServers = "No Authlib servers added yet.\nPlease go to Settings to add one.";
enData.accounts.authlibTab = "Authlib";
enData.accounts.goToSettings = "Go to Settings";

fs.writeFileSync(zhFile, JSON.stringify(zhData, null, 2) + '\n');
fs.writeFileSync(enFile, JSON.stringify(enData, null, 2) + '\n');

console.log('Locales updated successfully!');
