// CLI wrapper: parse a saved level page and print the solver payload JSON.
// Used by test_parse_parity.py to compare against the Python parser.
// Usage: node test_userscript_parse.js <level.html>

const fs = require('fs');
const { parseHtml } = require('./parse');

const html = fs.readFileSync(process.argv[2], 'utf8');
const parsed = parseHtml(html);
if (!parsed) {
    console.error('parse failed');
    process.exit(1);
}
console.log(JSON.stringify(parsed.payload));
