"""Capture tool versions, implementation hashes and read-only index freshness."""
from collections import Counter
from pathlib import Path
import platform
import sqlite3
import subprocess
from .core import digest, source_path


def version(argv):
    try:
        return subprocess.check_output(argv,text=True,stderr=subprocess.STDOUT,timeout=10).strip()
    except (OSError,subprocess.SubprocessError) as error:
        return str(error)


def implementation(base: Path, host: Path):
    files=sorted(base.glob('taskbench/*.py'))+sorted(base.glob('harness/src/*.rs'))
    return {'python':platform.python_version(),'platform':platform.platform(),
            'rg':version(['rg','--version']),'codegraph':version(['codegraph','--version']),
            'rustc':version(['rustc','--version']),
            'source_sha256':{str(p.relative_to(base)):digest(p.read_bytes()) for p in files},
            'host_sha256':digest(host.read_bytes()) if host.is_file() else None,
            'git_head':version(['git','-C',str(base),'rev-parse','HEAD'])}


def index_freshness(root: Path):
    database=root/'.codegraph/codegraph.db'
    if not database.is_file():
        return {'status':'unavailable'}
    counts=Counter(); examples=[]
    try:
        connection=sqlite3.connect(database.as_uri()+'?mode=ro',uri=True)
        try:
            rows=connection.execute('select path,content_hash from files').fetchall()
            for path,expected in rows:
                try:
                    state='matching' if digest(source_path(root,path).read_bytes())==expected else 'changed'
                except (ValueError,OSError):
                    state='missing_or_outside_root'
                counts[state]+=1
                if state!='matching': examples.append(path)
            return {'status':'fresh' if not examples else 'stale','counts':dict(counts),'mismatches':examples}
        finally:
            connection.close()
    except sqlite3.Error as error:
        return {'status':'unknown','error':str(error)}


def repository_snapshot(root: Path):
    """Hash tracked and untracked nonignored files, including dirty content."""
    from .core import canonical
    raw=subprocess.check_output(['git','ls-files','-z','--cached','--others','--exclude-standard'],cwd=root)
    entries={}
    for encoded in raw.split(b'\0'):
        if not encoded:
            continue
        relative=encoded.decode('utf-8')
        path=root/relative
        if path.is_symlink():
            entries[relative]='symlink:'+str(path.readlink())
        elif path.is_file():
            entries[relative]=digest(path.read_bytes())
        else:
            entries[relative]='missing-or-submodule'
    return {'scope':'git tracked + untracked nonignored files; symlinks recorded without following',
            'files':len(entries),'sha256':digest(canonical(entries).encode())}
