//! Revisions that give sub-agents the main conversation's tools, and the ones after them. Keep
//! each published identity and fingerprint immutable.
use super::PublishedRevision;

pub const PUBLISHED_SHARED: &[PublishedRevision] = &[
    // A picture file is read as the picture itself.
    PublishedRevision {
        name: "fs.read",
        revision: 8,
        tool: "tol_01M2DG020980080021AN942001",
        instance: "tin_01M2DG020980080021AN94SF08",
        fingerprint: "bb0be7a9fb7299eaadded5465079c2d6aa6c2975d8b39ef25f4e6e71105d3590",
    },
    // Given to sub-agents: their own history, the built-in browser on a page of their own, the tools that load others.
    PublishedRevision {
        name: "history.list_turns",
        revision: 3,
        tool: "tol_01M2DG020980080021AN942038",
        instance: "tin_01M2DG020980080021AN94SF09",
        fingerprint: "0384c63d375aaec1b21b54db15a17fe4e3d19ed645d9edb24758d302305c3ec9",
    },
    PublishedRevision {
        name: "history.search",
        revision: 3,
        tool: "tol_01M2DG020980080021AN942039",
        instance: "tin_01M2DG020980080021AN94SF0A",
        fingerprint: "a22cd5ef382541bc02b1814e89ab7478fa612a8f51d1f9184f2fd5d773e0b059",
    },
    PublishedRevision {
        name: "history.read",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94203A",
        instance: "tin_01M2DG020980080021AN94SF0B",
        fingerprint: "03784e7e1622d01c8d2b0c22a8097accd5e5f24957a50a70b4f1a20fef30f3fa",
    },
    PublishedRevision {
        name: "commands.list",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94201S",
        instance: "tin_01M2DG020980080021AN94SF0C",
        fingerprint: "32f062a77ac6086f4554167f256a6bfd5eb5f22edf0871fd2b9e8e45650895c4",
    },
    PublishedRevision {
        name: "commands.describe",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94201T",
        instance: "tin_01M2DG020980080021AN94SF0D",
        fingerprint: "1ece482a5e67e32c22f4a48a3d0371a23cf2ee2d6d54de43322e6adb17f11399",
    },
    PublishedRevision {
        name: "commands.save",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94201V",
        instance: "tin_01M2DG020980080021AN94SF0E",
        fingerprint: "674e038a2bafd115a1d5773b1862aeaacc4ce00abee1c0173ec9d33f63a1bfdd",
    },
    PublishedRevision {
        name: "commands.delete",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94201W",
        instance: "tin_01M2DG020980080021AN94SF0F",
        fingerprint: "ddabfbeda68778470a33fa4e50cbec16010a45bd034655efae3f29602597b683",
    },
    PublishedRevision {
        name: "plugins.validate",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94203B",
        instance: "tin_01M2DG020980080021AN94SF0G",
        fingerprint: "7370b951c888394a94c3d009bb2f1c9f54196af88eaf109cfc430c039b1a6803",
    },
    PublishedRevision {
        name: "plugins.dev_install",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94203C",
        instance: "tin_01M2DG020980080021AN94SF0H",
        fingerprint: "d9a314ad92e36b542c8cd453b4a9faba16a8dc14d3c8e3259ee3b1e5c856d777",
    },
    PublishedRevision {
        name: "plugins.logs",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94203D",
        instance: "tin_01M2DG020980080021AN94SF0J",
        fingerprint: "19a171a827fbb116ca6d2cc05ea776bd675f2f6280bcf75d9067399e16a965eb",
    },
    PublishedRevision {
        name: "tools.call",
        revision: 2,
        tool: "tol_01M2DG020980080021AN942023",
        instance: "tin_01M2DG020980080021AN94SF0K",
        fingerprint: "f7db6d0ce76060d834e230a93695c3d7aedb2fdb0335840411dec3e91ef54057",
    },
    PublishedRevision {
        name: "browser.open",
        revision: 4,
        tool: "tol_01M2DG020980080021AN942024",
        instance: "tin_01M2DG020980080021AN94SF0M",
        fingerprint: "9508b350b9a291a955a1c94498cf809d57dd6bd4daddbc9e063dcb5430ad3076",
    },
    PublishedRevision {
        name: "browser.navigate",
        revision: 3,
        tool: "tol_01M2DG020980080021AN942025",
        instance: "tin_01M2DG020980080021AN94SF0N",
        fingerprint: "6d20a2fa286b0df97452504a0a53055e25b7fd09235b6ebf5c4554e912bcf312",
    },
    PublishedRevision {
        name: "browser.snapshot",
        revision: 3,
        tool: "tol_01M2DG020980080021AN942026",
        instance: "tin_01M2DG020980080021AN94SF0P",
        fingerprint: "b614b16f9e9484be4faa9958e813f7623fb5bb049041c6d34d7beb4bfaabbaa7",
    },
    PublishedRevision {
        name: "browser.screenshot",
        revision: 3,
        tool: "tol_01M2DG020980080021AN942027",
        instance: "tin_01M2DG020980080021AN94SF0Q",
        fingerprint: "09a72d4c18dbe8c3aff3814677c1316ae6e31aac1ba1aade82014ab2de34d04d",
    },
    PublishedRevision {
        name: "browser.click",
        revision: 3,
        tool: "tol_01M2DG020980080021AN942028",
        instance: "tin_01M2DG020980080021AN94SF0R",
        fingerprint: "f19a71fde31a057e52b0b370547711acb37007940ce218b32a93b9b23063f6f3",
    },
    PublishedRevision {
        name: "browser.type",
        revision: 3,
        tool: "tol_01M2DG020980080021AN942029",
        instance: "tin_01M2DG020980080021AN94SF0S",
        fingerprint: "ba30ad6a8bfbfea2ed407f192e17759bc489e1d3bf47cfe33d4b76e3df52a53a",
    },
    PublishedRevision {
        name: "browser.press",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94202A",
        instance: "tin_01M2DG020980080021AN94SF0T",
        fingerprint: "58b6c888d2acb1059f2dbceb5f5a97c0c341caa13974f60b870d70b480976e05",
    },
    PublishedRevision {
        name: "browser.scroll",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94202B",
        instance: "tin_01M2DG020980080021AN94SF0V",
        fingerprint: "ae2ab35976dfa05f317e4c3366213f2f35bee9323aab8b1e5fb8530c47b4be1e",
    },
    PublishedRevision {
        name: "browser.hover",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94202C",
        instance: "tin_01M2DG020980080021AN94SF0W",
        fingerprint: "476cf3d86e2077ebbf851d50a473a91f4475d98f49cb87060467be023ec3708c",
    },
    PublishedRevision {
        name: "browser.select",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94202D",
        instance: "tin_01M2DG020980080021AN94SF0X",
        fingerprint: "dd53c8507ef8a9416c169c27d864c4890a2910768c68a0af715354fcfac6a3ad",
    },
    PublishedRevision {
        name: "browser.wait",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94202E",
        instance: "tin_01M2DG020980080021AN94SF0Y",
        fingerprint: "2f19cdfb4ac97a16a2eb0955f58f757621028a9bb86a96facebb8aeaf9e1a016",
    },
    PublishedRevision {
        name: "browser.console",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94202F",
        instance: "tin_01M2DG020980080021AN94SF0Z",
        fingerprint: "85ca4fd9bd0274240965b5072184c817e26053f8c52a487243379a237676a0c8",
    },
    PublishedRevision {
        name: "browser.close",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94202G",
        instance: "tin_01M2DG020980080021AN94SF10",
        fingerprint: "7c8154f53b0c511788ca8e9285765b9435b950b5d6cbe24922a67f79c5c0d23b",
    },
    PublishedRevision {
        name: "browser.evaluate",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94202J",
        instance: "tin_01M2DG020980080021AN94SF11",
        fingerprint: "7e2e5d5ae6e7ff1882a6611ff3a3230ccb28f034b47b71c9aeeabad7ab139f9a",
    },
    PublishedRevision {
        name: "browser.network",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94202K",
        instance: "tin_01M2DG020980080021AN94SF12",
        fingerprint: "c0329e9a6963b2f40187d7aaaf7f874416860d71a5caddb88b88b5ebb40d396f",
    },
    PublishedRevision {
        name: "browser.inspect",
        revision: 4,
        tool: "tol_01M2DG020980080021AN94202P",
        instance: "tin_01M2DG020980080021AN94SF13",
        fingerprint: "f0312bc397a2e09d378b0721da64111d6d38cdaad05378471de5203942912f0f",
    },
    PublishedRevision {
        name: "browser.performance",
        revision: 4,
        tool: "tol_01M2DG020980080021AN94202Q",
        instance: "tin_01M2DG020980080021AN94SF14",
        fingerprint: "5344f8fabde76562603a15cb4b16db413524942d161bac8696a70e285d32d617",
    },
    // A sub-agent has every tool unless narrowed, and starts in the mode the settings give.
    PublishedRevision {
        name: "agents.spawn",
        revision: 7,
        tool: "tol_01M2DG020980080021AN94203N",
        instance: "tin_01M2DG020980080021AN94SF15",
        fingerprint: "6ec2401c79f7a8f74a0084f09162bb108031b1666a6c1e884a7c51514c331b02",
    },
    // The main agent answers a sub-agent's question first, when the settings let it.
    PublishedRevision {
        name: "agents.approve",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94203Y",
        instance: "tin_01M2DG020980080021AN94SF16",
        fingerprint: "6745d378b279cf4a4b8f2424f4c4208567adf04e61c529d21157e4a1d7ca808b",
    },
    // agents.status and agents.wait list the questions the main agent answers first.
    PublishedRevision {
        name: "agents.status",
        revision: 4,
        tool: "tol_01M2DG020980080021AN94203P",
        instance: "tin_01M2DG020980080021AN94SF17",
        fingerprint: "28752afcb63ce15d638ef34143c9d32f694854470fa5c89dc241efd26ea6e820",
    },
    PublishedRevision {
        name: "agents.wait",
        revision: 4,
        tool: "tol_01M2DG020980080021AN94203Q",
        instance: "tin_01M2DG020980080021AN94SF18",
        fingerprint: "3c85548170f88836b7cfc4f359e92b1062b7fb065907744f47535e886a2cd5f7",
    },
    // Sub-agents have the main agent's tools and their own permission mode; its approvals may come to the main agent first.
    PublishedRevision {
        name: "agents.spawn",
        revision: 8,
        tool: "tol_01M2DG020980080021AN94203N",
        instance: "tin_01M2DG020980080021AN94SF19",
        fingerprint: "f81bf17b1d6efb2db21ea832e4337b42bfc5390ee9b7cb138cdcaa35c5a27384",
    },
    // fs.read names its read limits.
    PublishedRevision {
        name: "fs.read",
        revision: 9,
        tool: "tol_01M2DG020980080021AN942001",
        instance: "tin_01M2DG020980080021AN94SF1A",
        fingerprint: "0558031dac727dd1c83e206e00aa723e48ec9d71a13367f8c73d06ee84cf6360",
    },
    // The main agent may answer a question with the allowance the user's card offers for it.
    PublishedRevision {
        name: "agents.approve",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94203Y",
        instance: "tin_01M2DG020980080021AN94SF1B",
        fingerprint: "f9ed5e9f772cd6fa6d8d047d42221a4ff6c2b8f5729001396fe42245069d261a",
    },
    PublishedRevision {
        name: "agents.status",
        revision: 5,
        tool: "tol_01M2DG020980080021AN94203P",
        instance: "tin_01M2DG020980080021AN94SF1C",
        fingerprint: "314b7c85a5c444fad7b740d4209c21cfaff37906ae1d616cdf2b43366da08dbb",
    },
    PublishedRevision {
        name: "agents.wait",
        revision: 5,
        tool: "tol_01M2DG020980080021AN94203Q",
        instance: "tin_01M2DG020980080021AN94SF1D",
        fingerprint: "31da72b142867d8d52bf7b8783a8a0ae7719dcb489248f174b5e348909d84ea4",
    },
    // fs.read and fs.search say how their results read: a line of JSON, then numbered lines.
    PublishedRevision {
        name: "fs.read",
        revision: 10,
        tool: "tol_01M2DG020980080021AN942001",
        instance: "tin_01M2DG020980080021AN94SF1E",
        fingerprint: "a095db831f278b68ab80161cceac75c6a62ee2c432747aad2fece6134842610c",
    },
    PublishedRevision {
        name: "fs.search",
        revision: 10,
        tool: "tol_01M2DG020980080021AN942002",
        instance: "tin_01M2DG020980080021AN94SF1F",
        fingerprint: "870145ebea2eab20dd911b4705ad942ccd759aa0276d040f5ddfb5479f0ce8d7",
    },
    // fs.patch says that calls on one file in one response run in the order written, and that
    // several patches of it there may all give the hash the model read.
    PublishedRevision {
        name: "fs.patch",
        revision: 9,
        tool: "tol_01M2DG020980080021AN942004",
        instance: "tin_01M2DG020980080021AN94SF1G",
        fingerprint: "e49d40fbda422d93fc00a251d6a4ad22e2df852b6b64493a71312037a56eccd9",
    },
    // fs.search may show lines around its matches.
    PublishedRevision {
        name: "fs.search",
        revision: 11,
        tool: "tol_01M2DG020980080021AN942002",
        instance: "tin_01M2DG020980080021AN94SF1H",
        fingerprint: "89331b312e42d2f1e6aace8b0743fef34792280264cdf04dc6ecbad1665ddc44",
    },
    // agents.status and agents.wait show how a working sub-agent's turn is going.
    PublishedRevision {
        name: "agents.status",
        revision: 6,
        tool: "tol_01M2DG020980080021AN94203P",
        instance: "tin_01M2DG020980080021AN94SF1J",
        fingerprint: "9b6e9720099c68eb0a08627777c896f255a35bc85cf93812491f8dd2bc068090",
    },
    PublishedRevision {
        name: "agents.wait",
        revision: 6,
        tool: "tol_01M2DG020980080021AN94203Q",
        instance: "tin_01M2DG020980080021AN94SF1K",
        fingerprint: "96e8062ed8f2cbf5447589691b724c6360b0d0d926d2306a757797c6f2122328",
    },
    // tools.call says aura_purpose goes next to tool and arguments, once.
    PublishedRevision {
        name: "tools.call",
        revision: 3,
        tool: "tol_01M2DG020980080021AN942023",
        instance: "tin_01M2DG020980080021AN94SF1M",
        fingerprint: "c1851645c8e0a32f201a1e4c62b70cb2c4acfe17e8aa7b589ed5d6cea4b625b7",
    },
    // chrome.tabs says why its list is empty when Chrome is not connected.
    PublishedRevision {
        name: "chrome.tabs",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94202S",
        instance: "tin_01M2DG020980080021AN94SF1N",
        fingerprint: "1f52ea82ff2e258951fd961ce004ef3a20c89d59b2591425606c33c8475d4b56",
    },
    // terminal.read reads the lines the terminal shows, paged by line number; terminal.write
    // points to it.
    PublishedRevision {
        name: "terminal.read",
        revision: 3,
        tool: "tol_01M2DG020980080021AN942013",
        instance: "tin_01M2DG020980080021AN94SF1P",
        fingerprint: "3d61639113ca03e9151663fb0490392a5d72f3aa66ced3e4a15f0d57861e90e2",
    },
    PublishedRevision {
        name: "terminal.write",
        revision: 10,
        tool: "tol_01M2DG020980080021AN942012",
        instance: "tin_01M2DG020980080021AN94SF1Q",
        fingerprint: "b0f2e9971370fef9dfd1cf20ed01724b131337fb21fda8697de6d0a9553cab87",
    },
];
