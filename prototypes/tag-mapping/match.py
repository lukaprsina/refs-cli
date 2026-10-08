# PROTOTYPE for #58 - throwaway. Candidate tag matching over real `ls-remote --tags` lists.
# Needs <owner>_<repo>.txt files: git ls-remote --tags --refs https://github.com/<owner>/<repo>.git | sed "s#.*refs/tags/##"
import re
def cands(name, ver):
    bare = name.split('/')[-1]
    names = [name] + ([bare] if bare != name else [])
    out = [f"v{ver}", ver]
    for n in names:
        out += [f"{n}@{ver}", f"{n}-v{ver}", f"{n}-{ver}", f"{n}_v{ver}", f"{n}_{ver}"]
    seen=[]; [seen.append(c) for c in out if c not in seen]
    return seen
CASES = [
 ("tokio-rs_tokio","tokio","1.40.0"),("tokio-rs_tokio","tokio-util","0.7.12"),("tokio-rs_tokio","tokio","9.9.9"),
 ("rust-lang_log","log","0.4.22"),("serde-rs_serde","serde","1.0.200"),("serde-rs_serde","serde_derive","1.0.200"),
 ("TanStack_query","@tanstack/react-query","5.60.0"),("changesets_changesets","@changesets/cli","2.27.0"),
 ("solidjs_solid","solid-js","1.9.0"),("babel_babel","@babel/core","7.23.2"),("babel_babel","@babel/preset-env","7.23.2"),
 ("colinhacks_zod","zod","3.23.8"),("pallets_flask","flask","3.0.0"),("psf_requests","requests","2.31.0"),
 ("psf_requests","requests","2.31.0.post1"),("(no tags)","anything","1.0.0"),
]
for f,name,ver in CASES:
    tags = set(open(f+".txt").read().split()) if not f.startswith("(") else set()
    hits=[c for c in cands(name,ver) if c in tags]
    # loose: tags whose digits-suffix equals ver, to show what exact-only misses
    loose=sorted(t for t in tags if re.search(r'(^|[^0-9.])'+re.escape(ver)+r'$',t))
    print(f"{name}@{ver:<12} {f:<22} exact={hits} | any tag ending in version={loose[:6]}")
