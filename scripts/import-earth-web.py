#!/usr/bin/env python3
"""Build the neighbouring renderer and vendor its runtime, with exact source provenance."""
import hashlib,json,pathlib,subprocess,sys,datetime,shutil,tarfile
source=pathlib.Path(sys.argv[1] if len(sys.argv)>1 else '../cangling-desktop').resolve()
target=pathlib.Path(__file__).resolve().parents[1]/'ui/earth'
subprocess.run(['bash','scripts/build-web.sh'],cwd=source,check=True)
for name in ['navigation-mode.js','data-worker-client.js','data-worker.js','pkg/cangling_desktop.js','pkg/cangling_desktop_bg.wasm']:
 dst=target/name;dst.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(source/'web'/name,dst)
index=(source/'web/index.html').read_text().replace('<script type="module">','<script type="module" src="./keeper-bridge.js"></script>\n    <script type="module">',1)
(target/'index.html').write_text(index)
(target/'licenses').mkdir(exist_ok=True)
for p in (source/'assets').glob('*LICENSE.txt'):shutil.copy2(p,target/'licenses'/p.name)
hashfile=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
files=[source/'Cargo.toml',source/'Cargo.lock',source/'scripts/build-web.sh',source/'scripts/build-walker.py',source/'scripts/build-textured-building.py',source/'web/navigation-mode.js']+[p for folder in ['src','assets'] for p in (source/folder).rglob('*') if p.is_file()]
manifest={'builtAt':datetime.datetime.now(datetime.timezone.utc).isoformat(),'sourceCommit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=source,text=True).strip() if (source/'.git').exists() else None,'dirtyWorkingTree':bool(subprocess.check_output(['git','status','--porcelain'],cwd=source)) if (source/'.git').exists() else None, 'sourceSha256':{str(p.relative_to(source)):hashfile(p) for p in sorted(files)},'artifacts':{str(p.relative_to(target)):hashfile(p) for p in (target/'pkg').iterdir() if p.is_file()},'wasmBindgen':subprocess.check_output(['wasm-bindgen','--version'],text=True).strip()}
(target/'manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
print('Imported Earth runtime:',target)

# Keep the exact build input, including the renderer's pre-existing local features.
archive=target.parents[1]/'vendor/cangling-earth/source.tar.gz'
archive.parent.mkdir(parents=True,exist_ok=True)
with tarfile.open(archive,'w:gz') as tar:
 for p in files+[source/'web/index.html',source/'web/data-worker-client.js',source/'web/data-worker.js']:
  tar.add(p,arcname=str(p.relative_to(source)))
