// Production dependencies include optional/target edges. Eval is forbidden even in product tests.
export function dependencyGraph(metadata) {
  const members = new Set(metadata.workspace_members);
  return Object.fromEntries(
    metadata.packages
      .filter((pkg) => members.has(pkg.id))
      .sort((a, b) => a.name.localeCompare(b.name, 'en'))
      .map((pkg) => [
        pkg.name,
        [
          ...new Set(
            pkg.dependencies
              .filter((dep) => dep.kind !== 'dev' || dep.name === 'aura-tool-eval')
              .map((dep) => dep.name),
          ),
        ].sort(),
      ]),
  );
}

const kernelDependencies = {
  'aura-domain-common': [],
  'aura-agent-domain': ['aura-domain-common'],
  'aura-runtime-contract': ['aura-domain-common'],
  'aura-workspace-domain': ['aura-domain-common'],
  'aura-artifact-contract': ['aura-domain-common', 'aura-workspace-domain'],
  'aura-security-contract': ['aura-domain-common'],
  'aura-execution-contract': ['aura-domain-common', 'aura-security-contract'],
  'aura-windows-user-contract': [
    'aura-domain-common',
    'aura-security-contract',
    'aura-execution-contract',
  ],
  'aura-backend-windows-user': [
    'aura-domain-common',
    'aura-security-contract',
    'aura-execution-contract',
    'aura-windows-user-contract',
    'aura-backend-api',
    'aura-windows-platform',
  ],
  'aura-backend-api': ['aura-domain-common', 'aura-security-contract', 'aura-execution-contract'],
  'aura-execution-orchestrator': [
    'aura-backend-api',
    'aura-domain-common',
    'aura-execution-contract',
    'aura-security-contract',
    'aura-permission-engine',
    'aura-tool-contract',
    'aura-tool-domain',
    'aura-tool-projection',
    'aura-tool-scheduler',
  ],
  'aura-execution-maintainer': [
    'aura-domain-common',
    'aura-windows-user-contract',
    'aura-windows-platform',
  ],
  'aura-web-domain': ['aura-domain-common'],
  'aura-browser-domain': ['aura-domain-common'],
  'aura-web-runtime': ['aura-domain-common', 'aura-web-domain'],
  'aura-net-tools': [
    'aura-domain-common',
    'aura-web-domain',
    'aura-network-control',
    'aura-permission-domain',
    'aura-security-contract',
  ],
  'aura-backend-core': [
    'aura-domain-common',
    'aura-security-contract',
    'aura-execution-contract',
    'aura-backend-api',
  ],
  'aura-persistence-core': ['aura-domain-common'],
};

const isRunner = (name) =>
  /^aura-(?:command-runner|execution-helper|execution-maintainer|restricted-user-spike|sandbox-host|appcontainer-host|egress-proxy|plugin-host|browser-host)$/.test(
    name,
  );
const isPersistence = (name) =>
  name === 'rusqlite' || /^aura-(?:persistence(?:-core|-v2)?|.*-store)$/.test(name);

export function violations(graph) {
  const found = new Set();
  for (const [owner, dependencies] of Object.entries(graph)) {
    const allowed = kernelDependencies[owner];
    if (
      [
        'aura-core',
        'aura-command-runner',
        'aura-execution-helper',
        'aura-execution-maintainer',
        'aura-backend-windows-user',
      ].includes(owner) &&
      dependencies.includes('aura-restricted-user-spike')
    ) {
      found.add(`spike:${owner}->aura-restricted-user-spike`);
    }
    for (const dependency of dependencies) {
      if (dependency === 'git2' || dependency === 'libgit2-sys') {
        found.add(`git-native-owner:${owner}->${dependency}`);
      }
      if (allowed && dependency.startsWith('aura-') && !allowed.includes(dependency)) {
        found.add(`kernel:${owner}->${dependency}`);
      }
    }
    // Keep the entire dependency path: a legacy exemption cannot conceal a new route to SQL.
    function walk(name, path) {
      for (const dependency of graph[name] ?? []) {
        const next = [...path, dependency];
        if (
          owner.startsWith('aura-') &&
          owner !== 'aura-tool-eval' &&
          dependency === 'aura-tool-eval'
        ) {
          found.add(`evaluation:${next.join('->')}`);
        }
        if (isRunner(owner) && isPersistence(dependency)) {
          found.add(`runner:${next.join('->')}`);
        }
        if (owner === 'aura-backend-core' && isPersistence(dependency)) {
          found.add(`broker:${next.join('->')}`);
        }
        if (
          (owner === 'aura-persistence-v2' || owner.endsWith('-store')) &&
          (dependency === 'aura-persistence' || dependency === 'aura-v2-importer')
        ) {
          found.add(`v2-store:${next.join('->')}`);
        }
        if (path.includes(dependency)) {
          found.add(`cycle:${next.join('->')}`);
        } else if (Object.hasOwn(graph, dependency)) {
          walk(dependency, next);
        }
      }
    }
    walk(owner, [owner]);
  }
  return [...found].sort();
}

export function compareBaseline(graph, exceptions) {
  const current = violations(graph);
  const allowed = new Set(exceptions.map((item) => item.violation));
  return {
    added: current.filter((item) => !allowed.has(item)),
    remaining: current.filter((item) => allowed.has(item)),
    resolved: [...allowed].filter((item) => !current.includes(item)).sort(),
    pendingKernels: Object.keys(kernelDependencies).filter((name) => !Object.hasOwn(graph, name)),
  };
}
