// Compiler-only evidence for why the native loader callback needs authored text.
const fs = require('node:fs');
const crypto = require('node:crypto');
const compiler = process.argv[2];
const ts = require(compiler);
const cases = [
  { paths: { x: ['/src/x.js'] }, name: 'x', expected: '/src/x.js' },
  { paths: { '*': ['/src/*'] }, name: 'x.js', expected: '/src/x.ts' },
];
for (const row of cases) {
  row.files = ['/src/x.js', '/src/x.ts'];
  row.actual = ts.resolveModuleName(row.name, '/entry.ts', {
    moduleResolution: ts.ModuleResolutionKind.Bundler,
    paths: row.paths,
    pathsBasePath: '/',
  }, {
    fileExists: path => row.files.includes(path),
    readFile: () => undefined,
    directoryExists: () => true,
  }).resolvedModule?.resolvedFileName;
  if (row.actual !== row.expected) throw new Error(JSON.stringify(row));
}
console.log(JSON.stringify({
  scope: 'Compiler-only extension-priority observations, not native module-loader coverage',
  compiler_version: ts.version,
  compiler_sha256: crypto.createHash('sha256').update(fs.readFileSync(compiler)).digest('hex'),
  cases,
}, null, 2));
