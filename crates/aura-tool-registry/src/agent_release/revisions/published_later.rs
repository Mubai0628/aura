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
];
