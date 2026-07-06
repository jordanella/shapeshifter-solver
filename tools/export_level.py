"""Export a saved Shapeshifter HTML page to solver-core JSON input.

Usage: python tools/export_level.py <level.html> <out.json>
"""

import json
import sys

import shapeshifter_page as page


def main():
    html_path, out_path = sys.argv[1], sys.argv[2]
    html = open(html_path, encoding='utf-8', errors='ignore').read()
    d = page.parse_html(html)
    payload = page.to_payload(d)
    with open(out_path, 'w') as f:
        json.dump(payload, f)
    print(f'{html_path}: {payload["height"]}x{payload["width"]},'
          f' {payload["numStates"]} states,'
          f' {len(payload["shapes"])} shapes -> {out_path}')


if __name__ == '__main__':
    main()
