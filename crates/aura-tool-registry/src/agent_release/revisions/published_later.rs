//! Revisions published after the main table reached its size bound. Keep each published identity
//! and fingerprint immutable; a new revision of any tool is added here.
use super::PublishedRevision;

pub const PUBLISHED_LATER: &[PublishedRevision] = &[
    // `path_glob` without `/` names file names in any folder; files a partial search did not
    // reach may hold matches.
    PublishedRevision {
        name: "fs.search",
        revision: 9,
        tool: "tol_01M2DG020980080021AN942002",
        instance: "tin_01M2DG020980080021AN94SW02",
        fingerprint: "91123b7ae74c0b0a88edc3efa422bc3f93ac2e26ebf7d4fa292e7d28ad893163",
    },
    // A new file in folders that do not exist yet gets them.
    PublishedRevision {
        name: "fs.write",
        revision: 9,
        tool: "tol_01M2DG020980080021AN942003",
        instance: "tin_01M2DG020980080021AN94SW03",
        fingerprint: "e0621b3de079d6a127289472edd26800e43d86e2014b91ee7ea8c0295c0170a4",
    },
    // The activity query lists the calling session's own activities only.
    PublishedRevision {
        name: "agent.activity.query",
        revision: 4,
        tool: "tol_01ARZ3NDEKTSV4RRFFQ69G5FB0",
        instance: "tin_01M2DG020980080021AN94SW04",
        fingerprint: "cd4d984d75741f6b1c71be64917080d8572c714e0df589b368edbf1c651d0af2",
    },
    // A script that ends with a failing program reports that program's exit code.
    PublishedRevision {
        name: "shell.powershell",
        revision: 8,
        tool: "tol_01M2DG020980080021AN942005",
        instance: "tin_01M2DG020980080021AN94SW05",
        fingerprint: "c7d6ee0d8e52b2dfe71e1bed808f915283bf5afee934cfd393443c584a9ce3ab",
    },
    // A reference keeps its link to the prior record as derived_from_lineage_id, and says so.
    PublishedRevision {
        name: "task_lineage.reconcile",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94200N",
        instance: "tin_01M2DG020980080021AN94SW06",
        fingerprint: "2e4c8862f8efdf9d94b78425f460b680d9ccefb967c6b0641536afec89118f59",
    },
    // A command declares the credentials it needs; every credential's value is hidden in the output.
    PublishedRevision {
        name: "shell.powershell",
        revision: 9,
        tool: "tol_01M2DG020980080021AN942005",
        instance: "tin_01M2DG020980080021AN94SW07",
        fingerprint: "7c76cac2ecb77ca25a396d429c32ce2f23d7723078bd98411be8bad12a8a58f3",
    },
    // A command declares the credentials it needs; every credential's value is hidden in the output.
    PublishedRevision {
        name: "shell.cmd",
        revision: 7,
        tool: "tol_01M2DG020980080021AN942006",
        instance: "tin_01M2DG020980080021AN94SW08",
        fingerprint: "b0961e9203b043d398ebd0c4d3110afd9604fdeda0497c47bf35bd82216640e7",
    },
    // A terminal declares the credentials it needs; their values show as stars.
    PublishedRevision {
        name: "terminal.open",
        revision: 4,
        tool: "tol_01M2DG020980080021AN942011",
        instance: "tin_01M2DG020980080021AN94SW09",
        fingerprint: "d058504d369d34eb7ca9807cce239d8a34b2d91cedd93198918db3f6869bd6d7",
    },
    // The user's own Chrome, in tabs shared with the conversation.
    PublishedRevision {
        name: "chrome.tabs",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94202S",
        instance: "tin_01M2DG020980080021AN94SC01",
        fingerprint: "cd19ed1e54819ae5cb539fa07b7d00ca08ae061364ad5250719c477bc012af3e",
    },
    PublishedRevision {
        name: "chrome.open",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94202T",
        instance: "tin_01M2DG020980080021AN94SC02",
        fingerprint: "8f6bc1657e233b14bdc36fcf399acb0a31ebd9452be0b22c7d023c2f98544ad3",
    },
    PublishedRevision {
        name: "chrome.navigate",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94202V",
        instance: "tin_01M2DG020980080021AN94SC03",
        fingerprint: "0a50002621572f294d5bb8dbe4a1df5f9743346dcfd417f220a98150741896ac",
    },
    PublishedRevision {
        name: "chrome.snapshot",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94202W",
        instance: "tin_01M2DG020980080021AN94SC04",
        fingerprint: "9ba5027b2da418d77db59ac656bb92257e0310527a26a362370a43ab66e1340b",
    },
    PublishedRevision {
        name: "chrome.screenshot",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94202X",
        instance: "tin_01M2DG020980080021AN94SC05",
        fingerprint: "b1e61b78c21d0d2785898655592c16c8c50f5d7cb5ff4250a7aaac157f8ade8e",
    },
    PublishedRevision {
        name: "chrome.click",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94202Y",
        instance: "tin_01M2DG020980080021AN94SC06",
        fingerprint: "d086338105e7b5f5bee0a42b0b194eb4e03abca0d7c5df542c3662ae07d006e9",
    },
    PublishedRevision {
        name: "chrome.type",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94202Z",
        instance: "tin_01M2DG020980080021AN94SC07",
        fingerprint: "60c8314961c666b736f3b7393822011fbd59cc6ce7eeccbad552af6aa6f71e34",
    },
    PublishedRevision {
        name: "chrome.press",
        revision: 1,
        tool: "tol_01M2DG020980080021AN942030",
        instance: "tin_01M2DG020980080021AN94SC08",
        fingerprint: "991bc5874d101c5d652bc462afb392fa7bbd006bee291f14e38b8da83ecccc0b",
    },
    PublishedRevision {
        name: "chrome.scroll",
        revision: 1,
        tool: "tol_01M2DG020980080021AN942031",
        instance: "tin_01M2DG020980080021AN94SC09",
        fingerprint: "732b276006afd81a22355ab23f4878ce803030809976cc5f67920841118d3d19",
    },
    PublishedRevision {
        name: "chrome.hover",
        revision: 1,
        tool: "tol_01M2DG020980080021AN942032",
        instance: "tin_01M2DG020980080021AN94SC10",
        fingerprint: "b0ae1dc47d44d6d2b4180c4e3f9fe521d67ff7e8eb87d31918cf3c4558dedc74",
    },
    PublishedRevision {
        name: "chrome.select",
        revision: 1,
        tool: "tol_01M2DG020980080021AN942033",
        instance: "tin_01M2DG020980080021AN94SC11",
        fingerprint: "f295fcb7dac63cfb5482f33ef75e2dee3132b5303c7e18850c86bcdcd31ae8dd",
    },
    PublishedRevision {
        name: "chrome.wait",
        revision: 1,
        tool: "tol_01M2DG020980080021AN942034",
        instance: "tin_01M2DG020980080021AN94SC12",
        fingerprint: "284a9f70c437de05ddf3c8ccd9e1ae0930d123ee38d6f7b1afd6e06effa2e1c4",
    },
    PublishedRevision {
        name: "chrome.console",
        revision: 1,
        tool: "tol_01M2DG020980080021AN942035",
        instance: "tin_01M2DG020980080021AN94SC13",
        fingerprint: "ae431676f1f624d53c25352862443907681efee7851d4d638f2d1214ac0bc5b0",
    },
    PublishedRevision {
        name: "chrome.close",
        revision: 1,
        tool: "tol_01M2DG020980080021AN942036",
        instance: "tin_01M2DG020980080021AN94SC14",
        fingerprint: "1d7bf0c1c97b668721f0eeaeb0a3d9343286c2a6eef3683afc991e9150280348",
    },
    PublishedRevision {
        name: "chrome.request_user",
        revision: 1,
        tool: "tol_01M2DG020980080021AN942037",
        instance: "tin_01M2DG020980080021AN94SC15",
        fingerprint: "e96f3eda7255c42dd9be5732d47a032857f5aa963add77f5c4e9dc90f87f9a4d",
    },
    // A Shell result names its own tool_call_id, the id shell.query and shell.stop take; output
    // past max_output_bytes stops the command, and the result keeps the output up to then.
    PublishedRevision {
        name: "shell.powershell",
        revision: 10,
        tool: "tol_01M2DG020980080021AN942005",
        instance: "tin_01M2DG020980080021AN94SW10",
        fingerprint: "239280a698395aa05ecb5b16ee6048e21d8a375db3fd41ab5179a05b4ac81d37",
    },
    PublishedRevision {
        name: "shell.cmd",
        revision: 8,
        tool: "tol_01M2DG020980080021AN942006",
        instance: "tin_01M2DG020980080021AN94SW11",
        fingerprint: "178472f3f510790bb1b8f28187854deb35e6b89020aa92f531070c69c58ac9f5",
    },
    PublishedRevision {
        name: "shell.query",
        revision: 6,
        tool: "tol_01M2DG020980080021AN94200T",
        instance: "tin_01M2DG020980080021AN94SW12",
        fingerprint: "dd6ba9698e70bd9106bbcb021ab111e7eaee0f634c0a060d63b8bec43bcbb3e2",
    },
    PublishedRevision {
        name: "shell.stop",
        revision: 6,
        tool: "tol_01M2DG020980080021AN94200V",
        instance: "tin_01M2DG020980080021AN94SW13",
        fingerprint: "73caaed2d768beb339d776e3ff1502fefafa2cf8ed7640ac7918db8086850760",
    },
    // An event longer than the page comes with its length and no text, and is read in parts by
    // content_offset.
    PublishedRevision {
        name: "context.read_events",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94200A",
        instance: "tin_01M2DG020980080021AN94SW14",
        fingerprint: "a102cfb5332effb260cba640dff5440933d0f8d2cc778ba43f196def3aa2d643",
    },
    // A load or unload changes the list from the next turn; the chrome.* tools are the exception
    // the tool protocol names.
    PublishedRevision {
        name: "tools.load",
        revision: 4,
        tool: "tol_01ARZ3NDEKTSV4RRFFQ69G5FAX",
        instance: "tin_01M2DG020980080021AN94SW15",
        fingerprint: "5bdf746893348a775589445ddd97d21e7446c810986a8e6254ad20c07ae99888",
    },
    PublishedRevision {
        name: "tools.unload",
        revision: 4,
        tool: "tol_01ARZ3NDEKTSV4RRFFQ69G5FAZ",
        instance: "tin_01M2DG020980080021AN94SW16",
        fingerprint: "01681ecbc5fcd0c884cd36c920fec89dab0dbcf1f891bdebbdb35220682f9699",
    },
    // A site the conversation has a tab of opens again in that tab; tabs open and move in the
    // background.
    PublishedRevision {
        name: "chrome.open",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94202T",
        instance: "tin_01M2DG020980080021AN94SW17",
        fingerprint: "0c907ef777cb88b4a97b46f3736a5e1c9ebdf01c16a07d1725440ef10b640859",
    },
    // A tab Chrome does not draw is refused at once.
    PublishedRevision {
        name: "chrome.screenshot",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94202X",
        instance: "tin_01M2DG020980080021AN94SW18",
        fingerprint: "93c70a976c376c0d4e09b02396427ab9d698b73c465dda6ce50ba020e8e2f1b7",
    },
    // The conversation's history by round, read by entry.
    PublishedRevision {
        name: "history.list_turns",
        revision: 1,
        tool: "tol_01M2DG020980080021AN942038",
        instance: "tin_01M2DG020980080021AN94SW19",
        fingerprint: "b6bb657d2cbdbb34859bc34d3bc8498c76f2ba5f66af827bc756d3cb6d76035d",
    },
    PublishedRevision {
        name: "history.search",
        revision: 1,
        tool: "tol_01M2DG020980080021AN942039",
        instance: "tin_01M2DG020980080021AN94SW20",
        fingerprint: "2425baf118043a1fe10950f4cf681a0944708026b652b2c4fb25f46fe67d0a18",
    },
    PublishedRevision {
        name: "history.read",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94203A",
        instance: "tin_01M2DG020980080021AN94SW21",
        fingerprint: "4b44a0afc28df7fe47d9fb1a9e7763b3564ddb0a3d92963be2907c9fc931a768",
    },
    // Skills by name: a search gives each usable Skill's name and description, and a load gives
    // one Skill's instructions with its folder and files as a result in the conversation.
    PublishedRevision {
        name: "skills.search",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94201X",
        instance: "tin_01M2DG020980080021AN94SW22",
        fingerprint: "c72d59f4c4309854f36d8a6ca56022e2c31c379a47a42f5170e86b707d3edc6c",
    },
    PublishedRevision {
        name: "skills.load",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94201Z",
        instance: "tin_01M2DG020980080021AN94SW23",
        fingerprint: "e4e3f86695463bf7cb09bf899dc1c9f6305a07c4fe54f454eebef06fd228db39",
    },
    // The history of another conversation a user message referred to with "@", by its handle.
    PublishedRevision {
        name: "history.list_turns",
        revision: 2,
        tool: "tol_01M2DG020980080021AN942038",
        instance: "tin_01M2DG020980080021AN94SW24",
        fingerprint: "6e37507411932d69226201c1c6b25954aaf4056ae3b4d48a0eb1bc2927d6e97b",
    },
    PublishedRevision {
        name: "history.search",
        revision: 2,
        tool: "tol_01M2DG020980080021AN942039",
        instance: "tin_01M2DG020980080021AN94SW25",
        fingerprint: "077210831b54f48eec923b4ea69062fd3f68d178970b2ae23d98f01cc14091d5",
    },
    PublishedRevision {
        name: "history.read",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94203A",
        instance: "tin_01M2DG020980080021AN94SW26",
        fingerprint: "2afd9506f776d006cfdafc10062b2fa7674f151029e278126b0b2dad80f44ac1",
    },
    // Writing a plugin with the user: check its folder, install it in development, read its
    // errors.
    PublishedRevision {
        name: "plugins.validate",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94203B",
        instance: "tin_01M2DG020980080021AN94SW27",
        fingerprint: "64e3f302dbdca17518b978ef6ee851d6e7874ccf2729df429a1c8030e003c22e",
    },
    PublishedRevision {
        name: "plugins.dev_install",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94203C",
        instance: "tin_01M2DG020980080021AN94SW28",
        fingerprint: "75ea851645a77de159f3fe7fd5c5d00ae4bb8b3dae3625a87d69462368d5a6ff",
    },
    PublishedRevision {
        name: "plugins.logs",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94203D",
        instance: "tin_01M2DG020980080021AN94SW29",
        fingerprint: "c90f5cd232981b96bdf06765abdd17a7a97bed046e763287814cd8eedfddd1af",
    },
    // Programs on the user's own desktop that the user allowed.
    PublishedRevision {
        name: "computer.apps",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94203E",
        instance: "tin_01M2DG020980080021AN94SW2A",
        fingerprint: "c06666f2df05982bbef39845bd8c48dafd5da2aa0fc0a018873137bc7c64656f",
    },
    PublishedRevision {
        name: "computer.open",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94203F",
        instance: "tin_01M2DG020980080021AN94SW2B",
        fingerprint: "69c30ff84dc6fa560a02a219298d5de32ff3505c23ea5e5fe15173ec4074d839",
    },
    PublishedRevision {
        name: "computer.snapshot",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94203G",
        instance: "tin_01M2DG020980080021AN94SW2C",
        fingerprint: "c2307d960cb3cf5754f86460323f190885e787ec09947f857b48ead5f6afccfd",
    },
    PublishedRevision {
        name: "computer.screenshot",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94203H",
        instance: "tin_01M2DG020980080021AN94SW2D",
        fingerprint: "c63bd52fe05ccea56144df4c9ab3d5cb57e828f759c18d200ae7894a3d4436fd",
    },
    PublishedRevision {
        name: "computer.act",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94203J",
        instance: "tin_01M2DG020980080021AN94SW2E",
        fingerprint: "174f771aa5d6a8b1d6e240747ed7b721583ca62c5a0973bfa9fc75d263bf0dca",
    },
    PublishedRevision {
        name: "computer.input",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94203K",
        instance: "tin_01M2DG020980080021AN94SW2F",
        fingerprint: "5bb153ab6d4a9b89e48fbbc23f8ae5a6b7055877374670560af83c0c1a4e774f",
    },
    PublishedRevision {
        name: "computer.window",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94203M",
        instance: "tin_01M2DG020980080021AN94SW2G",
        fingerprint: "d3776a480d071995642a0a8ce8b26b927417d66343485c0088cc74b244325c93",
    },
    // The default area became the program's windows instead of the whole screen.
    PublishedRevision {
        name: "computer.screenshot",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94203H",
        instance: "tin_01M2DG020980080021AN94SW2H",
        fingerprint: "432fa42c378639212975d6d606cd2a911f1661b943ef750d1aa2bf34c485d12f",
    },
    // A program the user asked to be operated is operated with these tools; when the user did not
    // say how, the way that costs least time and tokens.
    PublishedRevision {
        name: "computer.apps",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94203E",
        instance: "tin_01M2DG020980080021AN94SW2J",
        fingerprint: "fe51010be278e72412b6ba428886f20b8426d2116640ca06b790857bf332e27d",
    },
    PublishedRevision {
        name: "computer.open",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94203F",
        instance: "tin_01M2DG020980080021AN94SW2K",
        fingerprint: "3cab79e5425e31ecbd2102928cf7b046fd3a7027151f1b00540d7a7335b95e9b",
    },
    PublishedRevision {
        name: "computer.snapshot",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94203G",
        instance: "tin_01M2DG020980080021AN94SW2M",
        fingerprint: "7eb98e3488f767d80267c956f2cd5b7912c7f3d6597ce02aeec4658a25400876",
    },
    PublishedRevision {
        name: "computer.screenshot",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94203H",
        instance: "tin_01M2DG020980080021AN94SW2N",
        fingerprint: "4a52eaa81dfad0ccd4a40d8f54876daf493cabc6183b39b6078e74adbccce0fa",
    },
    PublishedRevision {
        name: "computer.act",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94203J",
        instance: "tin_01M2DG020980080021AN94SW2P",
        fingerprint: "c8b31dc5578c5f22c148b863665df3ce9195e397c01d1c1fc2856491cb253d4f",
    },
    PublishedRevision {
        name: "computer.input",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94203K",
        instance: "tin_01M2DG020980080021AN94SW2Q",
        fingerprint: "80557a94b9eae41636de2619926ecab8dd81e1ac0a3961077712ab96543d7016",
    },
    PublishedRevision {
        name: "computer.window",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94203M",
        instance: "tin_01M2DG020980080021AN94SW2R",
        fingerprint: "5edd1ad51784adfc9e49dc974b904003b75f383d915f6db0d6196f1919c83bbf",
    },
    // Every call waits for a model step: one call carries the steps already known, and calls that
    // do not depend on each other go together. Opening a program brings its window to the front.
    PublishedRevision {
        name: "computer.apps",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94203E",
        instance: "tin_01M2DG020980080021AN94SW2S",
        fingerprint: "6d257b46a3c00a63a862261a9ec0871b7afc9ff0c3e57f954cdf1b3313d1fc47",
    },
    PublishedRevision {
        name: "computer.open",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94203F",
        instance: "tin_01M2DG020980080021AN94SW2T",
        fingerprint: "132eb698b845b9f829b9b29fe1869302d0eb3670642a8009893009b1aa3ee63b",
    },
    PublishedRevision {
        name: "computer.snapshot",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94203G",
        instance: "tin_01M2DG020980080021AN94SW2V",
        fingerprint: "7e82d4978d509b968461a8c60159946b5be657c662352a61664d1ee935fdcf29",
    },
    PublishedRevision {
        name: "computer.screenshot",
        revision: 4,
        tool: "tol_01M2DG020980080021AN94203H",
        instance: "tin_01M2DG020980080021AN94SW2W",
        fingerprint: "5d2a1462c33a982ad243135cb44a393297a85092dcb193eb8e55deeb6b41158c",
    },
    PublishedRevision {
        name: "computer.act",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94203J",
        instance: "tin_01M2DG020980080021AN94SW2X",
        fingerprint: "79a78b9a8ad4548b526057310bdc841f5c4d73dce1309a32b81ac7bd1447467a",
    },
    PublishedRevision {
        name: "computer.input",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94203K",
        instance: "tin_01M2DG020980080021AN94SW2Y",
        fingerprint: "9ba692f5837131e70b63e11c284206276b513c620c91507137c73d2ef80983cd",
    },
    PublishedRevision {
        name: "computer.window",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94203M",
        instance: "tin_01M2DG020980080021AN94SW2Z",
        fingerprint: "1d46a2cdb2b8fdc1f1eae8c9a630ca6dcb5098f1b536bbe2d15723f0b4face51",
    },
    // The way of working gives way to the user's: one call carries the steps already known unless
    // the user said otherwise, the mouse and keyboard are used when the user asked for them, and
    // nothing expanded has to be collapsed again.
    PublishedRevision {
        name: "computer.apps",
        revision: 4,
        tool: "tol_01M2DG020980080021AN94203E",
        instance: "tin_01M2DG020980080021AN94SW30",
        fingerprint: "b16971379414487e9e2dca08b338fa6bae8635f40575899e94258621d369d2c2",
    },
    PublishedRevision {
        name: "computer.open",
        revision: 4,
        tool: "tol_01M2DG020980080021AN94203F",
        instance: "tin_01M2DG020980080021AN94SW31",
        fingerprint: "8e632c5a07361b7af5c9fe9ee8643638f32d48568e4994583675d5e9ec191736",
    },
    PublishedRevision {
        name: "computer.snapshot",
        revision: 4,
        tool: "tol_01M2DG020980080021AN94203G",
        instance: "tin_01M2DG020980080021AN94SW32",
        fingerprint: "acde34eeda3100cba1d5f134b592f1007e2f1eabb6a027b1d65560e3c7d16fbd",
    },
    PublishedRevision {
        name: "computer.screenshot",
        revision: 5,
        tool: "tol_01M2DG020980080021AN94203H",
        instance: "tin_01M2DG020980080021AN94SW33",
        fingerprint: "d76a54ef2bf10fa6a1b5c7dbbe7b84aa36c85d9729fb38c47622a3ec0d5ca860",
    },
    PublishedRevision {
        name: "computer.act",
        revision: 4,
        tool: "tol_01M2DG020980080021AN94203J",
        instance: "tin_01M2DG020980080021AN94SW34",
        fingerprint: "b85c5292649a809663411718801711c4ece5dba4408db0600e450113e3a0ba06",
    },
    PublishedRevision {
        name: "computer.input",
        revision: 4,
        tool: "tol_01M2DG020980080021AN94203K",
        instance: "tin_01M2DG020980080021AN94SW35",
        fingerprint: "cb256ed4e3e2955da55ceb16f0aef7f3acfa888d6adf9e11df7b0154fd310a54",
    },
    PublishedRevision {
        name: "computer.window",
        revision: 4,
        tool: "tol_01M2DG020980080021AN94203M",
        instance: "tin_01M2DG020980080021AN94SW36",
        fingerprint: "38a95296ecb8eab06a4cd992c5a45a50003a46e2a8f85dba6195cfd72bb84359",
    },
    // Secrets are never stored; transient details and unverified claims are kept when the user
    // asks to keep them.
    PublishedRevision {
        name: "memory.create",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94201G",
        instance: "tin_01M2DG020980080021AN94SW37",
        fingerprint: "d5454f5eaec34c229553b6f1ec84ae6e3d113c003fee50cbc137de0d9a231fc4",
    },
];
