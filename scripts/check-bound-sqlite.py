#!/usr/bin/env python3
"""Bounded read-only verifier for the one pinned SQLite fork; never generates it."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import tomllib

UPSTREAM = '133c182a6a2c87864fe97778797e46c7e999672690dc9fa3ee8e241aa4a9c13f'
PATCHED = {'build.rs', 'sqlite3/sqlite3.c', 'sqlite3/sqlite3.h', 'sqlite3/bindgen_bundled_version.rs'}
FEATURES = {'default','min_sqlite_version_3_14_0','bundled','bundled_bindings','cc','pkg-config','vcpkg'}
SOURCE_ID = '2025-06-28 14:00:48 2af157d77fb1304a74176eaee7fbc7c7e932d946bf25325e9c26c91db19e3079'

def require(condition, message):
    if not condition:
        raise ValueError(message)

def safe(path):
    for ancestor in [path, *path.parents]:
        if ancestor.exists():
            s = ancestor.lstat()
            require(not stat.S_ISLNK(s.st_mode) and not getattr(s, 'st_file_attributes', 0) & 1024,
                    f'reparse/symlink refused: {ancestor}')
    return path

def read(path, limit=12_000_000):
    safe(path)
    require(path.stat().st_size <= limit, f'oversize: {path}')
    return path.read_bytes()

def digest(data):
    return {'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)}

def unix_abi_guard(source):
    marker = 'static int arxUnixNative(sqlite3_file *file, sqlite3_vfs *vfs){'
    require(source.count(marker) == 1, 'Unix native qualification definition')
    body = source.split(marker, 1)[1].split('\n}', 1)[0]
    require('return sizeof(void*)==8 && file && arxUnixVfs && vfs==arxUnixVfs' in body,
            'Unix qualification requires 64-bit pointer ABI')

def inventory(pins, observed):
    require(len(pins['original']) == 28, 'upstream count')
    require(set(pins['patched']) == PATCHED, 'patch footprint')
    require(set(observed) == set(pins['original']), 'member set')
    for name, original in pins['original'].items():
        require(observed[name] == pins['patched'].get(name, original), f'member mismatch: {name}')

def configs(root):
    paths = [a / '.cargo' / name for a in [root,*root.parents] for name in ['config','config.toml']]
    require('CARGO_HOME' in os.environ, 'explicit CARGO_HOME required')
    paths += [Path(os.environ['CARGO_HOME'])/name for name in ['config','config.toml']]
    for p in paths:
        if p.exists():
            obj = tomllib.loads(read(p, 1_000_000).decode())
            require(not any(k in obj for k in ['source','patch','replace']), f'Cargo substitution: {p}')
            for target in obj.get('target', {}).values():
                require('sqlite3' not in target, f'links override: {p}')
    for k in os.environ:
        require(not k.startswith(('CARGO_SOURCE_', 'CARGO_PATCH_','CARGO_REPLACE_')),
                'Cargo environment substitution')

def main():
    parser=argparse.ArgumentParser(__doc__)
    parser.add_argument('--metadata', required=True, type=Path, help='fresh cargo metadata --locked --offline JSON')
    parser.add_argument('--build-messages', required=True, type=Path, help='fresh cargo check --message-format=json output')
    parser.add_argument('--self-test', action='store_true')
    args=parser.parse_args()
    root=Path(__file__).resolve().parent.parent
    v=root/'vendor/libsqlite3-sys-0.35.0'
    pins=json.loads(read(v/'ARXCOVO-PINS.json',100_000))
    require(pins['archive_sha256']==UPSTREAM, 'archive checksum')
    files={}
    for count,p in enumerate(v.rglob('*'),1):
        require(count <= 64, 'vendor traversal bound exceeded')
        safe(p)
        if p.is_file():
            name=p.relative_to(v).as_posix()
            if name not in {'ARXCOVO-PATCH.md','ARXCOVO-PINS.json'}:
                files[name]=digest(read(p))
    inventory(pins,files)
    cargo=tomllib.loads(read(root/'Cargo.toml',100_000).decode())
    require(cargo['workspace']['exclude']==['vendor/libsqlite3-sys-0.35.0'], 'workspace exclude')
    require(cargo['patch']['crates-io']['libsqlite3-sys']=={'path':'vendor/libsqlite3-sys-0.35.0'}, 'Cargo patch')
    lock=tomllib.loads(read(root/'Cargo.lock',200_000).decode())
    packages=[p for p in lock['package'] if p['name']=='libsqlite3-sys']
    require(packages==[{'name':'libsqlite3-sys','version':'0.35.0','dependencies':['cc','pkg-config','vcpkg']}], 'lock package')
    for name in ['sqlite3/sqlite3.c','sqlite3/sqlite3.h']:
        data=read(v/name).decode()
        require(SOURCE_ID in data and '#define ARX_FCNTL_IDENTITY_V1 0x41525831' in data
                and 'sqlite3_arxcovo_qualify_main_v1' in data and 'sqlite3_arxcovo_bound_abi_v1' in data,
                'ABI/source declaration')
    configs(root)
    native_source=read(v/'sqlite3/sqlite3.c').decode()
    unix_abi_guard(native_source)
    metadata=json.loads(read(args.metadata,20_000_000))
    packages=[p for p in metadata['packages'] if p['name']=='libsqlite3-sys']
    require(len(packages)==1 and Path(packages[0]['manifest_path']).resolve()==(v/'Cargo.toml').resolve(), 'effective fork path')
    require(packages[0]['source'] is None and packages[0]['links']=='sqlite3', 'effective package')
    node=next(n for n in metadata['resolve']['nodes'] if n['id']==packages[0]['id'])
    require(set(node['features'])==FEATURES, 'effective features')
    messages=[json.loads(line) for line in read(args.build_messages,20_000_000).splitlines() if line.strip()]
    builds=[b for b in messages if b.get('package_id')==packages[0]['id'] and b.get('reason')=='build-script-executed']
    require(len(builds)==1, 'effective build-script metadata missing')
    build=builds[0]
    require(build['linked_libs']==['static=sqlite3'] and not build['cfgs'] and not build['env'], 'effective link mode')
    out=Path(build['out_dir']).resolve()
    require(out.is_relative_to(Path(os.environ['CARGO_TARGET_DIR']).resolve()), 'foreign build output')
    candidates=[]
    for entry in build['linked_paths']:
        require(entry.startswith('native='), 'unsupported link search')
        directory=Path(entry.removeprefix('native='))
        for name in ['sqlite3.lib','libsqlite3.a','sqlite3.dll','libsqlite3.so','libsqlite3.dylib']:
            candidate=directory/name
            if candidate.exists():
                safe(candidate)
                require(directory.resolve()==out and name in ['sqlite3.lib','libsqlite3.a'], 'SQLite library substitution')
                candidates.append(candidate)
    # cc emits identical .lib/.a aliases on MSVC; distinct content still refuses.
    require(1 <= len(candidates) <= 2 and len({hashlib.sha256(read(p)).digest() for p in candidates})==1, 'missing/ambiguous static SQLite library')
    negatives=0
    if args.self_test:
        # Width policy checks are source controls, not native ILP32 execution.
        for width in ['4', '16']:
            wrong=native_source.replace('sizeof(void*)==8', f'sizeof(void*)=={width}')
            try: unix_abi_guard(wrong)
            except ValueError: negatives+=1
            else: raise ValueError('unsupported Unix ABI width accepted')
        wrong=native_source.replace('sizeof(void*)==8 && ', '')
        try: unix_abi_guard(wrong)
        except ValueError: negatives+=1
        else: raise ValueError('missing Unix ABI width accepted')
        # Pure inventory negative controls: no source mutation or product execution.
        for name in sorted(files):
            wrong=dict(files); wrong[name]={'sha256':'0'*64,'bytes':0}
            try: inventory(pins,wrong)
            except ValueError: negatives+=1
            else: raise ValueError('lost source accepted')
        for wrong in [dict(list(files.items())[1:]),{**files,'unexpected':digest(b'')}]:
            try: inventory(pins,wrong)
            except ValueError: negatives+=1
            else: raise ValueError('changed footprint accepted')
    print(f'PASS: 28 members, four patched hashes, Cargo path/lock/features/config; {negatives} pure inventory negative controls')
    print('Trusted toolchain only: compile commands/config CLI and compiler identities require recorded qualification evidence; inventory is not linked-binary proof.')

if __name__=='__main__':
    main()
