#!/usr/bin/env python3
"""R6 evidence: parse raw command logs, never synthesize test/diagnostic counts."""
import argparse, hashlib, json, re, subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def fingerprint(root=ROOT):
    names = subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard','-z'], cwd=root).decode().split('\0')
    # Markdown/evidence/bundles/generated platform schemas are separately sealed.
    # Product, tests, tools, config and lockfiles all participate in the tested hash.
    files = {}
    for name in sorted(set(names)):
        if not name or (name.startswith(('artifacts/','docs/','macos/src-tauri/gen/','windows/src-tauri/gen/')) and name != 'artifacts/composite-repair/2026-10-04-r0/expected-failures.json') or name.endswith(('.md','.app')): continue
        path = root/name
        if path.is_file(): files[name] = digest(path)
    aggregate = hashlib.sha256(''.join(f'{n}\0{h}\n' for n,h in files.items()).encode()).hexdigest()
    return dict(schema=1, head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(), source_sha256=aggregate,
                method='SHA256(sorted relative path + NUL + content SHA256 hex + LF)',
                scope='product/tests/tools/config/lockfiles and the canonical expected-failures ledger; docs/Markdown, other artifacts, generated platform schemas excluded and sealed separately', files=files,
                dirty_diff_sha256=hashlib.sha256(subprocess.check_output(['git','diff','HEAD','--binary'],cwd=root)).hexdigest(),
                git_status=subprocess.check_output(['git','status','--porcelain=v1'],cwd=root,text=True).splitlines(),
                untracked_source_files=[n for n in subprocess.check_output(['git','ls-files','--others','--exclude-standard','-z'],cwd=root).decode().split('\0') if n in files])

def parse_log(text):
    # ANSI styling is not a diagnostic identity.
    text = re.sub(r'\x1b\[[0-9;]*m','',text)
    warnings = {}
    for line in text.splitlines():
        try: obj=json.loads(line)
        except (ValueError, TypeError): continue
        if not isinstance(obj,dict) or obj.get('reason')!='compiler-message': continue
        m=obj['message']
        if m.get('level')!='warning': continue
        spans=[]
        for span in m.get('spans',[]):
            if not span.get('is_primary'): continue
            primary={k:span.get(k) for k in ('file_name','line_start','column_start','line_end','column_end')}
            name=primary.get('file_name');manifest=obj.get('manifest_path')
            if name and not re.match(r'^[A-Za-z]:[\\/]',name):
                if Path(name).is_absolute(): primary['file_name']=str(Path(name).resolve())
                elif manifest: primary['file_name']=str((Path(manifest).parent/name).resolve())
            spans.append(primary)
        identity=dict(code=(m.get('code') or {}).get('code'),message=m.get('message'),spans=spans)
        if not spans: identity['package_id']=obj.get('package_id')
        warnings[json.dumps(identity,sort_keys=True)] = identity
    # Human Cargo summaries are explicitly not additional diagnostics.
    # Non-JSON tool warnings retain their complete distinct line identity.
    tool_warnings=sorted({line.strip() for line in text.splitlines() if re.match(r'^\s*(?:warning:|WARNING:)',line)
                         and not re.search(r'generated \d+ warnings?|duplicate|build failed, waiting',line)})
    rust=[dict(passed=int(a),failed=int(b),ignored=int(c),filtered=int(d)) for a,b,c,d in re.findall(
        r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; \d+ measured; (\d+) filtered out;',text)]
    swift={}
    for name,outcome in re.findall(r"Test Case '([^']+)' (passed|failed|skipped)",text): swift[name]=outcome
    vitest=re.findall(r'^\s*Tests\s+([^\n]+)',text,re.M)
    tap=re.findall(r'(?:^# |^ℹ )tests (\d+)\s*$',text,re.M)
    counts=dict(passed=sum(r['passed'] for r in rust), failed=sum(r['failed'] for r in rust), skipped=sum(r['ignored'] for r in rust))
    if swift: counts=dict(passed=sum(v=='passed' for v in swift.values()),failed=sum(v=='failed' for v in swift.values()),skipped=sum(v=='skipped' for v in swift.values()))
    elif vitest:
        counts={kind:sum(int(n) for line in vitest for n in re.findall(r'(\d+) '+kind,line)) for kind in ['passed','failed','skipped']}
    elif re.search(r'Ran (\d+) tests? in ', text):
        count=int(re.search(r'Ran (\d+) tests? in ',text).group(1));failed=sum(int(n) for n in re.findall(r'(?:failures|errors)=(\d+)',text));skipped=sum(int(n) for n in re.findall(r'skipped=(\d+)',text));counts=dict(passed=count-failed-skipped,failed=failed,skipped=skipped)
    elif tap:
        failed=int((re.findall(r'(?:^# |^ℹ )fail (\d+)',text,re.M) or ['0'])[-1]);skipped=int((re.findall(r'(?:^# |^ℹ )skipped (\d+)',text,re.M) or ['0'])[-1]);counts=dict(passed=int(tap[-1])-failed-skipped,failed=failed,skipped=skipped)
    return dict(tests=counts, executed_tests=counts['passed']+counts['failed'], rust_suites=rust,
                unique_compiler_warnings=len(warnings),diagnostics=list(warnings.values()), tool_warning_lines=tool_warnings)

def generate(run, write=True):
    checks=[]
    for file in sorted((run/'commands').glob('*.json')):
        record=json.loads(file.read_text()); log=run/record['log']; parsed=parse_log(log.read_text(errors='replace'))
        errors=[]
        if record['exit_code']!=record.get('expected_exit',0): errors.append('unexpected exit code')
        if record.get('min_tests') and parsed['executed_tests']<record['min_tests']: errors.append('required tests did not execute')
        if record.get('exact_tests') is not None and parsed['executed_tests']!=record['exact_tests']: errors.append('filter test count mismatch')
        if parsed['tests']['failed']: errors.append('test failures in log')
        if record['id'] == 'regressions':
            ledger=run/'regressions/results.json'
            if ledger.exists():
                nested=json.loads(ledger.read_text());record['regression_checks']=len(nested['checks']);record['regression_index_sha256']=digest(ledger)
                record['regression_logs']={str(p.relative_to(run)):digest(p) for p in sorted((run/'regressions/logs').glob('*.log'))}
                record['regression_results']=nested['checks']
            else: errors.append('missing regression result index')
        if '--skip' in record['command']: errors.append('skip flag forbidden in R6')
        irohs=re.findall(r'test (iroh_transport::[^ ]+) \.\.\. ([^\n]+)', log.read_text(errors='replace'))
        record['iroh_tests']=[dict(name=n,outcome=v) for n,v in irohs]
        if any('ignored' in v for n,v in irohs): errors.append('Iroh test skipped')
        record.update(parsed,log_sha256=digest(log),status='FAIL' if errors else 'PASS',validity_errors=errors)
        checks.append(record)
    unique={json.dumps(d,sort_keys=True):d for c in checks for d in c['diagnostics']}
    out=dict(schema=1, run_id=run.name, source_sha256=json.loads((run/'source-fingerprint.json').read_text())['source_sha256'],
             checks=checks, unique_compiler_warnings=len(unique), unexpected=sum(c['status']=='FAIL' for c in checks),
             release='NO-GO', native_windows='UNKNOWN', target_apps='UNKNOWN', two_devices='UNKNOWN',
             note='Generated from immutable command records and raw logs. Retries never replace the first failure. Host tests do not certify Windows native behavior.')
    if write: (run/'results.json').write_text(json.dumps(out,indent=2)+'\n')
    return out


def require(condition, message):
    if not condition:
        raise ValueError(message)


def contained(run, name):
    path = (run / name).resolve()
    require(path.is_relative_to(run.resolve()), f'evidence path escapes run: {name}')
    return path


def verify_r6(run, sealed=True, source=None, expected=None):
    """Read-only verification. Counts and commands come from the current runner.

    Never repair/reseal evidence while consuming it, and never trust a persisted
    reduced matrix, a PASS string, or a source hash without its file inventory.
    """
    run = Path(run).resolve()
    source = fingerprint() if source is None else source
    fp = json.loads((run / 'source-fingerprint.json').read_text())
    require(fp['source_sha256'] == source['source_sha256'] and fp['files'] == source['files'],
            'tested source changed or source inventory differs')
    if expected is None:
        expected = json.loads(subprocess.check_output(
            ['python3', str(ROOT / 'scripts/run-composite-r6.py'), run.name, '--list-json'],
            cwd=ROOT, text=True))
    specs = {c['id']: c for c in expected}
    matrix = json.loads((run / 'matrix.json').read_text())
    require(matrix['required'] == list(specs), 'required matrix differs from current runner')
    parsed = generate(run, write=False)
    records = parsed['checks']
    require({c['id'] for c in records} == set(specs), 'missing or unknown required commands')
    require(parsed['unexpected'] == 0, 'an attempted R6 command failed')
    for c in records:
        spec = specs[c['id']]
        require(c.get('source_sha256') == source['source_sha256'], f"{c['id']}: source mismatch")
        require(c.get('expected_exit', 0) == 0, f"{c['id']}: altered expected exit")
        for key in ('command', 'environment', 'min_tests', 'exact_tests'):
            require(c.get(key) == spec.get(key), f"{c['id']}: altered {key}")
        require(contained(run, c['log']).is_file(), f"{c['id']}: missing raw log")
    require(json.loads((run / 'results.json').read_text()) == parsed,
            'stored results differ from raw command records/logs')
    by_id = {c['id']: c for c in records}
    for name in ('core-default', 'core-enabled', 'core-repair'):
        if name in by_id:
            tests = by_id[name]['iroh_tests']
            require(bool(tests) and all(t['outcome'].strip() == 'ok' for t in tests),
                    f'{name}: Iroh tests did not pass')
    # Auxiliary records must bind the same candidate, with nonempty required scopes.
    for name in ('package-manifest', 'fault-matrix', 'rollback-proof', 'package-probes'):
        data = json.loads((run / (name + '.json')).read_text())
        require(data.get('source_sha256') == source['source_sha256'], f'{name}: source mismatch')
        if name == 'fault-matrix':
            require(len(data.get('cases', [])) >= 13 and all(c['status'] == 'PASS' for c in data['cases']),
                    'fault matrix incomplete or failed')
        elif name != 'package-manifest':
            require(data.get('status') == 'PASS', f'{name}: not PASS')
    packages = json.loads((run / 'package-manifest.json').read_text())['packages']
    required_kinds = {'default-native', 'default-universal-apple-darwin',
                      'enabled-native', 'enabled-universal-apple-darwin', 'debug'}
    require({p['kind'] for p in packages if p.get('status') == 'PASS'} == required_kinds,
            'required package variants incomplete')
    for package in packages:
        if package['kind'] not in required_kinds:
            continue
        require(package['source_sha256'] == source['source_sha256'], 'package source mismatch')
        inventory = package['all_files_sha256']
        require(bool(inventory), 'empty package inventory')
        # Every inspected byte must live in this run, including debug binaries.
        # Build caches and symlinks into them are mutable, unsealed dependencies.
        base = contained(run, package['path'])
        require(base.is_dir(), 'package snapshot directory missing')
        require(all(not Path(name).is_absolute() for name in inventory),
                'package inventory must use relative paths')
        actual = {str(p.relative_to(base)) for p in base.rglob('*') if p.is_file()}
        require(actual == set(inventory), 'package inventory incomplete')
        for name, sha in inventory.items():
            path = contained(base, name)
            require(path.is_file() and digest(path) == sha, f'package bytes changed: {path}')
        aggregate = hashlib.sha256(''.join(f'{n}\0{h}\n' for n, h in inventory.items()).encode()).hexdigest()
        require(aggregate == package['package_sha256'], 'package aggregate mismatch')
    if sealed:
        status = json.loads((run / 'status.json').read_text())
        require(status.get('state') == 'LOCAL_COMPLETE' and status.get('source_sha256') == source['source_sha256'],
                'R6 is not sealed LOCAL_COMPLETE for this source')
        index = json.loads((run / 'evidence-index.json').read_text())
        require(index.get('source_sha256') == source['source_sha256'], 'seal source mismatch')
        inventory = index['files']
        actual = {str(p.relative_to(run)) for p in run.rglob('*') if p.is_file()
                  and p.name != 'evidence-index.json' and not any(x.endswith('.app') for x in p.relative_to(run).parts)}
        require(actual == set(inventory), 'sealed inventory incomplete')
        for name, sha in inventory.items():
            path = contained(run, name)
            require(path.is_file() and digest(path) == sha, f'sealed evidence changed: {name}')
    return parsed

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('run',type=Path);p.add_argument('--fingerprint',action='store_true');p.add_argument('--verify',action='store_true');a=p.parse_args()
    if a.verify:
        try:
            data=verify_r6(a.run);print(f"VALID: {len(data['checks'])} commands, current source and sealed evidence match")
        except (ValueError, KeyError, OSError) as error:
            print(f'INVALID: {error}');raise SystemExit(1)
    elif a.fingerprint:
        a.run.mkdir(parents=True,exist_ok=True); data=fingerprint();(a.run/'source-fingerprint.json').write_text(json.dumps(data,indent=2)+'\n');print(data['source_sha256'])
    else:
        data=generate(a.run);print(f"{len(data['checks'])} commands, {data['unexpected']} unexpected, {data['unique_compiler_warnings']} unique compiler warnings");raise SystemExit(bool(data['unexpected']))
