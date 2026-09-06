# The Linux package repositories

Schemaic publishes a signed **APT** repository and a signed **DNF/zypper**
repository at <https://fadion.github.io/schemaic>, so a `.deb` or `.rpm` install
is carried forward by `apt-get upgrade` / `dnf upgrade` instead of by re-running
`install.sh`. This directory builds them; `.github/workflows/pages.yml` runs it
after every tagged release.

```
build-site.sh        the whole site: collects packages, calls the two below,
                     writes the client config, the key and the landing page
build-apt-repo.sh    pool/ + dists/ + a signed InRelease
build-rpm-repo.sh    signed packages + repodata/ + a signed repomd.xml.asc
verify-site.sh       reads a built site back the way apt and dnf would
verify-site.py       the index half of that check
index.html.in        the landing page, with __BASE_URL__ / __VERSION__ /
                     __FINGERPRINT__ filled in at build time
```

## The shape of it

**The GitHub Release assets are the source of truth; the site is derived.** Each
run downloads the `.deb` and `.rpm` from the five most recent releases and
rebuilds everything from nothing.

That is what keeps this cheap to own. There is no `gh-pages` branch — a branch
accumulating ~40 MB of packages per release would be cloned by everyone who ever
clones this repository, forever, and deleting the files later would not shrink a
single clone that already exists. An Actions deployment has no git history, so
the published size is the only size: five releases is about 205 MB against
GitHub Pages' 1 GB limit, with a 100 GB/month bandwidth allowance above it.

It also means **a repository that has gone wrong is repaired by running the
workflow again**, from the Actions tab. There is no accumulated state to unpick,
because the previous run's output is never read.

## One-time setup

### 1. The signing key

Not optional, and not the same thing as the code signing this project has
deliberately gone without. A GPG key here says *this package came from this
repository and arrived unaltered* — it vouches for no identity, costs nothing,
and is what `apt` and `dnf` actually check. The alternative is not "an unsigned
repository": it is telling every user to write `[trusted=yes]`, which is a worse
posture than the direct download it would be replacing. So the workflow refuses
to publish without one.

On any machine with `gpg` (WSL will do), in an isolated keyring so this never
lands in your personal one:

```bash
export GNUPGHOME="$(mktemp -d)" && chmod 700 "$GNUPGHOME"
echo allow-loopback-pinentry > "$GNUPGHOME/gpg-agent.conf"

# Pick a passphrase and keep it; you will need it again below.
printf '%s' 'YOUR-PASSPHRASE' > "$GNUPGHOME/pass" && chmod 600 "$GNUPGHOME/pass"

gpg --batch --pinentry-mode loopback --passphrase-file "$GNUPGHOME/pass" \
    --quick-generate-key "Schemaic package signing <fadion@users.noreply.github.com>" \
    rsa4096 sign never
```

`rsa4096` rather than an elliptic-curve key because RHEL-era `rpm` cannot verify
EdDSA signatures, and `never` rather than an expiry because an expired key breaks
`apt update` for every user on a date rather than on a release — a silent,
scheduled outage with nothing to do about it but rotate.

Then export it and hand both halves to Actions:

```bash
fpr=$(gpg --list-secret-keys --with-colons | awk -F: '/^fpr:/ {print $10; exit}')
gpg --batch --pinentry-mode loopback --passphrase-file "$GNUPGHOME/pass" \
    --armor --export-secret-keys "$fpr" > schemaic-signing-key.asc

gh secret set GPG_PRIVATE_KEY --repo fadion/schemaic < schemaic-signing-key.asc
gh secret set GPG_PASSPHRASE --repo fadion/schemaic   # paste the passphrase
```

**Back `schemaic-signing-key.asc` up somewhere you will still have it in two
years, then delete the working copy.** Losing it means generating a new key, and
every machine that has ever installed from this repository then fails
verification on the next `apt update` until its user imports the replacement by
hand. There is no way to reach those people to tell them.

The public key is not committed here on purpose. The site exports it from the
private key on every run, so there is exactly one copy and nothing that can
quietly disagree with what is actually signing the packages.

**The fingerprint is a deliberate exception, and rotating the key means editing
the top-level README.** It is printed there because the by-hand `dnf` route asks
the user to approve a fingerprint, and one they can only compare against the
same server the key arrived from is not a comparison — this repository's history
is a channel that server does not control. That only works while the two agree,
so a rotation updates the README in the same commit, or the independent channel
becomes an independently wrong answer.

### 2. Pages

Once, so that deployments from Actions are accepted:

```bash
gh api -X POST repos/fadion/schemaic/pages -f build_type=workflow
```

Or Settings → Pages → Source → **GitHub Actions**.

### 3. First publish

The workflow runs itself after the next tagged release. To stand the site up
before then, run **Pages** from the Actions tab — it builds from releases that
already exist, so it needs no new tag.

## Testing it

`verify-site.sh` runs in the workflow before anything is deployed, and it is the
only check there is. What it catches is not a wrong answer from a function but
metadata that disagrees with the packages beside it — a `Packages` file naming a
`.deb` that was pruned, a `Release` still describing the previous run's
`Packages.gz`. Every tool involved reports success for those, and they surface on
a user's machine at install time. So the check is the one a client performs:
verify the signatures against the *published* public key, then confirm every file
the indexes name exists and hashes to what they claim.

To build the site by hand — needs `gh`, `apt-utils`, `createrepo-c`, `rpm`,
`gpg` and `python3`:

```bash
export GPG_KEY_ID=<fingerprint> GPG_PASSPHRASE_FILE=/path/to/pass
export SCHEMAIC_REPO_URL=http://localhost:8000 SCHEMAIC_RETAIN=2
bash packaging/repo/build-site.sh /tmp/site
bash packaging/repo/verify-site.sh /tmp/site
```

To then install from it the way a user would, serve it and point a container at
it — this is the end-to-end check, and the only one that exercises `install.sh`:

```bash
(cd /tmp/site && python3 -m http.server 8000) &
docker run --rm -it --network host debian:12 bash -c '
  apt-get update && apt-get install -y curl ca-certificates
  curl -fsSL http://localhost:8000/schemaic-archive-keyring.gpg \
    -o /usr/share/keyrings/schemaic-archive-keyring.gpg
  curl -fsSL http://localhost:8000/schemaic.sources \
    -o /etc/apt/sources.list.d/schemaic.sources
  apt-get update && apt-get install -y schemaic && schemaic --version'
```

## What users do with it

Documented on the generated landing page and in the top-level README. The parts
that are interface, and cannot be reworded without breaking somebody:

- `https://fadion.github.io/schemaic/deb` and `/rpm` — written into every user's
  source list. Moving either one stops updates for every existing install,
  silently. This is the same class of permanence as the Velopack channel names.
- `Origin: Schemaic`, `Suite: stable` — what a Debian user writes in
  `Unattended-Upgrade::Allowed-Origins` as `"Schemaic:stable"`.
- `/usr/share/keyrings/schemaic-archive-keyring.gpg` — the path `Signed-By`
  names in the published `.sources` file.

Nothing upgrades on its own without the user asking, on either family. What the
repositories buy is that asking is the command they already run for everything
else.
