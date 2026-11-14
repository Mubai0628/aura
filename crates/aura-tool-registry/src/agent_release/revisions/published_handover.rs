//! Revisions of the tools that also act on what the user handed to the conversation with @. Keep
//! each published identity and fingerprint immutable.
use super::PublishedRevision;

pub const PUBLISHED_HANDOVER: &[PublishedRevision] = &[
    // The user's own terminal, once the user hands it over.
    PublishedRevision {
        name: "terminal.open",
        revision: 5,
        tool: "tol_01M2DG020980080021AN942011",
        instance: "tin_01M2DG020980080021AN94SX01",
        fingerprint: "0aeea2d221389e5f6196ad312d0e90dc618eb35b61e016935f209356b694fed1",
    },
    PublishedRevision {
        name: "terminal.write",
        revision: 5,
        tool: "tol_01M2DG020980080021AN942012",
        instance: "tin_01M2DG020980080021AN94SX02",
        fingerprint: "933b709bc44c3fee995ecf110ae27d779caeef5490efc66c9f0f4d8da10beecd",
    },
    PublishedRevision {
        name: "terminal.read",
        revision: 2,
        tool: "tol_01M2DG020980080021AN942013",
        instance: "tin_01M2DG020980080021AN94SX03",
        fingerprint: "d7ad30bd8bf53dfdbd0258200494459453e86ed582e04b118e974d619ec27371",
    },
    PublishedRevision {
        name: "terminal.close",
        revision: 2,
        tool: "tol_01M2DG020980080021AN942014",
        instance: "tin_01M2DG020980080021AN94SX04",
        fingerprint: "7a77645ca2568c7e3243cdc5da8e9bd7b24d4367cb31bc304f3fe1f8661dff2d",
    },
    PublishedRevision {
        name: "terminal.request_sensitive_input",
        revision: 2,
        tool: "tol_01M2DG020980080021AN942015",
        instance: "tin_01M2DG020980080021AN94SX05",
        fingerprint: "177737f3b43dd0fc3f33eb1f4f43e4d3d692be8c40e4472e7410e6bb2eeac9f7",
    },
    // A page of the user's in the built-in browser, once the user hands it over.
    PublishedRevision {
        name: "browser.open",
        revision: 2,
        tool: "tol_01M2DG020980080021AN942024",
        instance: "tin_01M2DG020980080021AN94SY01",
        fingerprint: "97c118911ba248386968df006bbda16e6a308de730dae7e46ac0ef6bc9033b14",
    },
    PublishedRevision {
        name: "browser.navigate",
        revision: 2,
        tool: "tol_01M2DG020980080021AN942025",
        instance: "tin_01M2DG020980080021AN94SY02",
        fingerprint: "2034f22bcbddf0a3dd1634230ed7a4aad56957ac3e669ce04738e502ddcdf73a",
    },
    PublishedRevision {
        name: "browser.snapshot",
        revision: 2,
        tool: "tol_01M2DG020980080021AN942026",
        instance: "tin_01M2DG020980080021AN94SY03",
        fingerprint: "8b14af137849a5c93dbb33a28fc43fa890222630dbd6c5e570a391f206ab0fcb",
    },
    PublishedRevision {
        name: "browser.screenshot",
        revision: 2,
        tool: "tol_01M2DG020980080021AN942027",
        instance: "tin_01M2DG020980080021AN94SY04",
        fingerprint: "a0281e4574cbf48750e5cf98a13d756bc266da54e33a5eea71817e5ff5b01ec1",
    },
    PublishedRevision {
        name: "browser.click",
        revision: 2,
        tool: "tol_01M2DG020980080021AN942028",
        instance: "tin_01M2DG020980080021AN94SY05",
        fingerprint: "c8c7119edb715c913ca6b86c0487c3bf4184540d542a0c79e380bb83f9a01edc",
    },
    PublishedRevision {
        name: "browser.type",
        revision: 2,
        tool: "tol_01M2DG020980080021AN942029",
        instance: "tin_01M2DG020980080021AN94SY06",
        fingerprint: "84b8b9af62211ae50e4af53b54c8517fef721856acddae2c6739f71ef8d6aa24",
    },
    PublishedRevision {
        name: "browser.press",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94202A",
        instance: "tin_01M2DG020980080021AN94SY07",
        fingerprint: "5e028fa823a1426585f2a4efdc965fe5955171a41d7a2ab4f050220aa06955db",
    },
    PublishedRevision {
        name: "browser.scroll",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94202B",
        instance: "tin_01M2DG020980080021AN94SY08",
        fingerprint: "2ff46012cd37dc052205e93acd373d3599dfa70a7767990ac4bb5873ff533914",
    },
    PublishedRevision {
        name: "browser.hover",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94202C",
        instance: "tin_01M2DG020980080021AN94SY09",
        fingerprint: "84da8c995aea0d78172200a46934d6f245f9f0a93c9d11ec90baa042237d54ca",
    },
    PublishedRevision {
        name: "browser.select",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94202D",
        instance: "tin_01M2DG020980080021AN94SY10",
        fingerprint: "1038c090e695e188220f35bf7c34822cf718602ceda7f1ac09228ced6d63db71",
    },
    PublishedRevision {
        name: "browser.wait",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94202E",
        instance: "tin_01M2DG020980080021AN94SY11",
        fingerprint: "f75578e1df508920a64e0d9b336cbf07af193adcfc2e6cbb7a3e0aff7890e0a5",
    },
    PublishedRevision {
        name: "browser.console",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94202F",
        instance: "tin_01M2DG020980080021AN94SY12",
        fingerprint: "84516fe47884bee7c0030c947851f1c668d974f5f477e0333e0a58a8579cf77a",
    },
    PublishedRevision {
        name: "browser.close",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94202G",
        instance: "tin_01M2DG020980080021AN94SY13",
        fingerprint: "eac371160f808884475632201d9cc392e29609c17d96c026461baa525f7e8e30",
    },
    PublishedRevision {
        name: "browser.request_user",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94202H",
        instance: "tin_01M2DG020980080021AN94SY14",
        fingerprint: "02f31a9bb4845a0d2012ba6975eb652d2489d518bed260c7081ce02930a1913f",
    },
    PublishedRevision {
        name: "browser.evaluate",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94202J",
        instance: "tin_01M2DG020980080021AN94SY15",
        fingerprint: "8eeb8a82f6e0103d41fb34cb5c6a7a0fa652237488825f3e4bbed7bf527514a6",
    },
    PublishedRevision {
        name: "browser.network",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94202K",
        instance: "tin_01M2DG020980080021AN94SY16",
        fingerprint: "ec425f8dbf7dfcf8879b693a6e5716375150dd392bf7f7ec71a28793b3955187",
    },
    PublishedRevision {
        name: "browser.inspect",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94202P",
        instance: "tin_01M2DG020980080021AN94SY17",
        fingerprint: "cb007e4a1162095d9296444df41d8b1e22a06b94aa775355774cabfd371bf2d3",
    },
    PublishedRevision {
        name: "browser.performance",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94202Q",
        instance: "tin_01M2DG020980080021AN94SY18",
        fingerprint: "0853b2aad212f3057b45848f3e53b8fb0e2000ad9f25e9a9e836279b8d2fb0f8",
    },
    // A write returns once the output has been quiet for a moment, waiting up to 15 s by default.
    PublishedRevision {
        name: "terminal.write",
        revision: 6,
        tool: "tol_01M2DG020980080021AN942012",
        instance: "tin_01M2DG020980080021AN94SF02",
        fingerprint: "93fc08330e0300e1f8d0f0081f1595c1ee1c2d0d3a365f6f37f10c2db9091bd6",
    },
    // Input is typed as given, so a newline at its end runs the command line.
    PublishedRevision {
        name: "terminal.write",
        revision: 7,
        tool: "tol_01M2DG020980080021AN942012",
        instance: "tin_01M2DG020980080021AN94SF03",
        fingerprint: "664ccf76bf1e1cc8eca7c5f52d959a2903fba30d0018d508d08de8ab9015793e",
    },
    // A write answers once the shell's prompt is back, else after 3 seconds of quiet.
    PublishedRevision {
        name: "terminal.write",
        revision: 8,
        tool: "tol_01M2DG020980080021AN942012",
        instance: "tin_01M2DG020980080021AN94SF04",
        fingerprint: "d4f720cb4d5c48e8833900d8ccb77cb76d51634f4d5a8df85ef1bc217756c1be",
    },
    // An until_text is looked for after the typed line's echo; the prompt and the quiet answer only
    // a write without one.
    PublishedRevision {
        name: "terminal.write",
        revision: 9,
        tool: "tol_01M2DG020980080021AN942012",
        instance: "tin_01M2DG020980080021AN94SF05",
        fingerprint: "60a2849deafe486d94aa5803de3ebac465acfa52b747c4810552763ac058a98f",
    },
    // A category's form is described: 1 to 64 lowercase ASCII letters, digits or '-'.
    PublishedRevision {
        name: "memory.create",
        revision: 4,
        tool: "tol_01M2DG020980080021AN94201G",
        instance: "tin_01M2DG020980080021AN94SF06",
        fingerprint: "d23cce3a4fa23dbc9c63a8a2c76daac53996d5286df036c56c5214d91b312192",
    },
    PublishedRevision {
        name: "memory.update",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94201H",
        instance: "tin_01M2DG020980080021AN94SF07",
        fingerprint: "875edb2409a2d53f54d38e7839f8139898873d6f65cbd8ac2a7bc452ce1f2b6f",
    },
    // The main agent's browser opens a file of its sub-agent's worktree.
    PublishedRevision {
        name: "browser.open",
        revision: 3,
        tool: "tol_01M2DG020980080021AN942024",
        instance: "tin_01M2DG020980080021AN94SY1N",
        fingerprint: "cb79e11055ef17241d78e161357549e3a37fcf9f1cde22cdd5b279cf242b8da5",
    },
];
