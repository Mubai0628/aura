//! Sub-agent release facts. Keep each published identity and fingerprint immutable.
use super::PublishedRevision;

pub const PUBLISHED_TEAM: &[PublishedRevision] = &[
    PublishedRevision {
        name: "agents.spawn",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94203N",
        instance: "tin_01M2DG020980080021AN94SW38",
        fingerprint: "8568423e94485d47e3da6331d54769a4f8d4f7e6e613d8ebc40d60391049c6e9",
    },
    PublishedRevision {
        name: "agents.status",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94203P",
        instance: "tin_01M2DG020980080021AN94SW39",
        fingerprint: "96c79194774e6f0540784901c7defade7fd3487b0bb8e60dfcf4f10cb44d62f8",
    },
    PublishedRevision {
        name: "agents.wait",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94203Q",
        instance: "tin_01M2DG020980080021AN94SW3A",
        fingerprint: "f47ee704d665f4d99af7f0a343134ac64ba8eb68850c7bfaa2b03b96ab3db9af",
    },
    PublishedRevision {
        name: "agents.send",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94203R",
        instance: "tin_01M2DG020980080021AN94SW3B",
        fingerprint: "63f2f842bd4b3c2dbf81d99e2cdbe97ebe3dc561aa55131e936ea4480601bd62",
    },
    PublishedRevision {
        name: "agents.result",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94203S",
        instance: "tin_01M2DG020980080021AN94SW3C",
        fingerprint: "bf01f4e757b661c0a54b563f0e60bed19d06e740be694b040de004ca2a7391e1",
    },
    PublishedRevision {
        name: "agents.stop",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94203T",
        instance: "tin_01M2DG020980080021AN94SW3D",
        fingerprint: "f0e39fca299f4cf1ebb0260406cf38f8ac823a505933a72e0067428968321eaf",
    },
    PublishedRevision {
        name: "agent.report",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94203V",
        instance: "tin_01M2DG020980080021AN94SW3E",
        fingerprint: "3c35083e7179501067852500519a7f819e954614210a9802f02a3ca84a3cb1ff",
    },
    // A sub-agent may work in a Git worktree of its own; the main agent is told the files two of
    // them changed, and merges or discards a worktree.
    PublishedRevision {
        name: "agents.spawn",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94203N",
        instance: "tin_01M2DG020980080021AN94SW3F",
        fingerprint: "20677a69392739f5725a109e8826ecca11d8350b35991d94057404443944e13f",
    },
    PublishedRevision {
        name: "agents.status",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94203P",
        instance: "tin_01M2DG020980080021AN94SW3G",
        fingerprint: "0547382809067f032abd084ff22209afa6b3aa2469a4acc4d73fa95615bff208",
    },
    PublishedRevision {
        name: "agents.wait",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94203Q",
        instance: "tin_01M2DG020980080021AN94SW3H",
        fingerprint: "27c3e0e6bf8966b125db94a2b715f4cfcef6ccc2631a85a4994ffed57a595981",
    },
    PublishedRevision {
        name: "agents.merge",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94203W",
        instance: "tin_01M2DG020980080021AN94SW3J",
        fingerprint: "8fa154842a6a85c91f9d4385ad10824ec2ae8e168d6a44254598f466ab94f264",
    },
    PublishedRevision {
        name: "agents.discard",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94203X",
        instance: "tin_01M2DG020980080021AN94SW3K",
        fingerprint: "9c01e3a77ec9130ab4b5103e0738f34b1b70001872d0e76c63f78a952c86f555",
    },
    // The user sets how many work at once, their token budget and their time per task; the
    // watchdog stops one past either, and one whose budget is used up takes no more messages.
    PublishedRevision {
        name: "agents.spawn",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94203N",
        instance: "tin_01M2DG020980080021AN94SW3M",
        fingerprint: "d8c3b84d4342018995f01e3bb066b1289aea08d492cb4a1616d86d84fc786143",
    },
    PublishedRevision {
        name: "agents.send",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94203R",
        instance: "tin_01M2DG020980080021AN94SW3N",
        fingerprint: "998ae3b7d766bdc99f2b326c38f35ddb2470ede42317d892ab33fbcdea8effad",
    },
    // A sub-agent in a worktree keeps its writing tools after a message; a report's summary is
    // a summing-up of at most 8000 characters, and long content goes in files or the answer. The
    // tools that return reports carry the summary's new description.
    PublishedRevision {
        name: "agents.send",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94203R",
        instance: "tin_01M2DG020980080021AN94SY19",
        fingerprint: "2b672842defbb6d44b66e11b3bb75c78fadb15ab3dcd90245fd1de0103966f2d",
    },
    PublishedRevision {
        name: "agent.report",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94203V",
        instance: "tin_01M2DG020980080021AN94SY1A",
        fingerprint: "4cad9dbe299661876c56241c4d1d45cd89c6423d9fb23f03fe4ed6bdb5d94542",
    },
    PublishedRevision {
        name: "agents.status",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94203P",
        instance: "tin_01M2DG020980080021AN94SY1B",
        fingerprint: "1e830922e8554e54d2c30b0a994bdf35142dbce40e2f77ccc6fab8573b1a4dae",
    },
    PublishedRevision {
        name: "agents.wait",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94203Q",
        instance: "tin_01M2DG020980080021AN94SY1C",
        fingerprint: "147c533c051fbc8dfa173767d1699e91cd57045e4efb972684dc76afb72c7044",
    },
    PublishedRevision {
        name: "agents.result",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94203S",
        instance: "tin_01M2DG020980080021AN94SY1D",
        fingerprint: "f44837425500b1c6403d5376c83797e8468d2ca185e5400ccf99c85e686e7ce1",
    },
    // A sub-agent also sees the user's Skills and loads them itself.
    PublishedRevision {
        name: "agents.spawn",
        revision: 4,
        tool: "tol_01M2DG020980080021AN94203N",
        instance: "tin_01M2DG020980080021AN94SY1E",
        fingerprint: "599786139ccc14b22408f654fccd1b90a118716712f653ca86796fa361917f4c",
    },
    // A sub-agent may be given the Skill tools: no longer the main agent's alone.
    PublishedRevision {
        name: "skills.search",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94201X",
        instance: "tin_01M2DG020980080021AN94SY1F",
        fingerprint: "6557d79897e33691d07a031a8124ff3d8f0c720f0678db1d95d1694e3f53e26b",
    },
    PublishedRevision {
        name: "skills.load",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94201Z",
        instance: "tin_01M2DG020980080021AN94SY1G",
        fingerprint: "1e50c476b64c41cbf0e3cbeb67362d08655947aac382d288700f8c3b7d73d3cb",
    },
    // A sub-agent's final answer is its report; agent.report tells progress or asks a question.
    // The Skill tools come with every sub-agent, and skills.search finds Skills by any of its
    // words.
    PublishedRevision {
        name: "agent.report",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94203V",
        instance: "tin_01M2DG020980080021AN94SY1H",
        fingerprint: "931da8655e3a90a9a3a7aa1dbd42df0fe2645d24d32b1bbff730393b85bf4e9b",
    },
    PublishedRevision {
        name: "agents.spawn",
        revision: 5,
        tool: "tol_01M2DG020980080021AN94203N",
        instance: "tin_01M2DG020980080021AN94SY1J",
        fingerprint: "111bae8581692beebc10dce0b08dae7346fb4b82c7410338effef3e2168b0bde",
    },
    PublishedRevision {
        name: "agents.result",
        revision: 3,
        tool: "tol_01M2DG020980080021AN94203S",
        instance: "tin_01M2DG020980080021AN94SY1K",
        fingerprint: "ecfa447e48c4e8af8973f873668e46671c6cc85f08988ca5194eb5d25897d138",
    },
    PublishedRevision {
        name: "skills.search",
        revision: 4,
        tool: "tol_01M2DG020980080021AN94201X",
        instance: "tin_01M2DG020980080021AN94SY1M",
        fingerprint: "f120ccd510912c14647a2d3d771a9ac3726fa24e15406c86f96589d2e2084ec5",
    },
    // A worktree takes the workspace's uncommitted changes along, and a merge into a workspace
    // with its own uncommitted changes writes the sub-agent's changes into its files.
    PublishedRevision {
        name: "agents.spawn",
        revision: 6,
        tool: "tol_01M2DG020980080021AN94203N",
        instance: "tin_01M2DG020980080021AN94SY1P",
        fingerprint: "2b3356051481b4d893ea5b688ce650af0f7f95998113c45615185198614af48b",
    },
    PublishedRevision {
        name: "agents.merge",
        revision: 2,
        tool: "tol_01M2DG020980080021AN94203W",
        instance: "tin_01M2DG020980080021AN94SY1Q",
        fingerprint: "a74f271034366e1de5d965cd52be6f3a965e760f249ad391a5c71acf119371a2",
    },
];
