"""在独立本机 PostgreSQL 和真实 Docker Sidecar 中验收文件整包生命周期。"""
import base64
import hashlib
import json
import os
import socket
import shutil
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from pathlib import Path

plugin = Path(__file__).resolve().parents[2]
binary = Path(os.environ['AIO_FILE_DELIVERY_HOST']).resolve(strict=True)
database = os.environ['AIO_FILE_DELIVERY_DATABASE_URL']
parsed = urllib.parse.urlparse(database)
assert parsed.hostname == '127.0.0.1' and parsed.path == '/file_delivery_test'
postgres = os.environ['AIO_FILE_DELIVERY_POSTGRES_CONTAINER']
web = Path(os.environ['AIO_FILE_DELIVERY_WEB']).resolve(strict=True)
packager = Path(os.environ['AIO_FILE_DELIVERY_PACKAGER']).resolve(strict=True)
root = Path(tempfile.mkdtemp(prefix='aio-file-delivery-'))
(root / 'processes').mkdir()
(root / 'config.toml').write_text('plugins = []\n', encoding='utf-8')
with socket.socket() as reservation:
    reservation.bind(('127.0.0.1', 0))
    port = reservation.getsockname()[1]
base = 'http://127.0.0.1:%d' % port
image = 'sha256:48a661b5e5f4560287dfc254e948825538d0f7a1379616495a424ed375afd31f'
env = {k: v for k, v in os.environ.items() if not k.startswith('AIO_')}
env.update({
    'AIO_DATABASE_URL': database, 'AIO_COMPONENT_DATABASE_URL': database,
    'AIO_COMPONENT_HOME': str(root / 'components'), 'AIO_PLUGIN_CACHE': str(root / 'cache'),
    'AIO_CONFIG': str(root / 'config.toml'),
    'AIO_PROCESS_ROOT': str(root / 'processes'), 'AIO_PROCESS_IMAGES': image,
    'AIO_PLUGIN_SUPERVISOR_SOCKET': str(root / 'supervisor.sock'),
    'AIO_PUBLIC_ORIGIN': base, 'AIO_WEB_HOST': '127.0.0.1', 'AIO_WEB_PORT': str(port),
    'AIO_WEB_DIST': str(web), 'AIO_BOOTSTRAP_ACCOUNT': 'file-test',
    'AIO_BOOTSTRAP_PASSWORD': uuid.uuid4().hex, 'AIO_PLUGIN_PUBLISH_ACCOUNTS': 'file-test',
})
http = urllib.request.build_opener(urllib.request.ProxyHandler({}))
cookie = None
host = None
supervisor = None
host_log = open(str(root / 'host.log'), 'w', encoding='utf-8')
supervisor_log = open(str(root / 'supervisor.log'), 'w', encoding='utf-8')
host_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
png = base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9Zl1sAAAAASUVORK5CYII=')


def sql(statement):
    return subprocess.check_output(['docker', 'exec', '-i', postgres, 'psql', '-U', parsed.username, '-d', 'file_delivery_test', '-X', '-v', 'ON_ERROR_STOP=1', '-Atc', statement], universal_newlines=True).strip()


def request(path, value=None, raw=None):
    headers = {'Cookie': cookie} if cookie else {}
    body = raw
    if raw is not None:
        headers['Content-Type'] = 'application/vnd.aio.component+gzip'
    if value is not None:
        body = json.dumps(value).encode('utf-8')
        headers['Content-Type'] = 'application/json'
    try:
        response = http.open(urllib.request.Request(base + path, data=body, headers=headers), timeout=180)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        return response.status, response.headers, response.read()


def success(path, value=None, raw=None):
    status, _, body = request(path, value, raw)
    assert 200 <= status < 300, '%s: HTTP %s %s' % (path.split('/')[3:5], status, body[:250])
    if not body:
        return None
    return json.loads(body).get('data')


def until(predicate, timeout=60):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.1)
    raise AssertionError('等待超时；检查日志目录 ' + str(root))


def ready():
    assert host.poll() is None, '宿主提前退出；检查 ' + str(root)
    try:
        return request('/health')[0] == 200
    except OSError:
        return False


def launch():
    global host
    host = subprocess.Popen([str(binary)], cwd=str(root), env=env, stdout=host_log, stderr=subprocess.STDOUT)
    until(ready)


def stop():
    global host
    if host is not None and host.poll() is None:
        host.terminate()
        assert host.wait(timeout=40) == 0
    host = None


def pages():
    return success('/api/runtime/catalog')['pages']


def mount(source, version=None):
    value = success('/api/runtime/frontend/mount', {'page_id': 'component:%s:file-list' % source})
    if version is not None:
        path = urllib.parse.urlsplit(value['src'])
        status, _, body = request(path.path + ('?' + path.query if path.query else ''))
        assert status == 200 and ('data-delivery-version="%s"' % version).encode() in body
    return value['token']


def invoke(token, method, path, value=None, raw=None, expected=None):
    payload = {'method': method, 'path': path, 'headers': [], 'body': []}
    if value is not None:
        payload['headers'] = [{'name': 'content-type', 'value': 'application/json'}]
        raw = json.dumps(value).encode('utf-8')
    if raw is not None:
        payload['body'] = list(raw)
    response = success('/api/runtime/components/%s/request' % token, payload)
    if expected is not None:
        assert response['status'] == expected, response
    else:
        assert 200 <= response['status'] < 300, response
    return json.loads(bytes(response['body']).decode('utf-8')).get('data') if response['body'] else None


def download(token, item):
    query = urllib.parse.urlencode({'path': '/files/' + item['id'], 'name': item['name']})
    return request('/api/runtime/components/%s/download?%s' % (token, query))


def upload(token, name, size, mime='application/octet-stream'):
    session = invoke(token, 'POST', '/uploads', {'name': name, 'content_type': mime, 'size_bytes': size})
    digest = hashlib.sha256()
    for index in range(session['chunk_count']):
        body = bytes([index % 256]) * min(session['chunk_bytes'], size - index * session['chunk_bytes'])
        if index == 0 and mime == 'image/png':
            body = png + body[len(png):]
        digest.update(body)
        invoke(token, 'PUT', '/uploads/%s/chunks/%s' % (session['id'], index), raw=body)
    item = invoke(token, 'POST', '/uploads/%s/complete' % session['id'])
    assert item['sha256'] == digest.hexdigest()
    return item


def publish(version):
    bundle = root / ('file-%s.aio-plugin' % version)
    commit = os.environ.get('AIO_FILE_DELIVERY_COMMIT') or subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=str(plugin), universal_newlines=True).strip()
    candidate = root / ('candidate-' + version)
    candidate.mkdir()
    shutil.copy2(str(plugin / 'aio-plugin.toml'), str(candidate / 'aio-plugin.toml'))
    shutil.copytree(str(plugin / 'dist'), str(candidate / 'dist'))
    shutil.copytree(str(plugin / 'server/migrations'), str(candidate / 'server/migrations'))
    index = candidate / 'dist/frontend/index.html'
    index.write_text(index.read_text(encoding='utf-8').replace('<body>', '<body data-delivery-version="%s">' % version), encoding='utf-8')
    subprocess.check_call([str(packager), str(candidate), 'aio-plugin.toml', 'https://github.com/org-aio/aio-plugin-file.git', commit, version, str(bundle)])
    result = success('/api/runtime/components/publish', raw=bundle.read_bytes())
    print('Published file ' + version, flush=True)
    return result


def migrate(source):
    instances = []
    for config in (root / 'processes').glob('*/grant/config.json'):
        value = json.loads(config.read_text(encoding='utf-8'))
        if value['tenant_id'] == 'default' and value.get('public_base') == '/api/runtime/components/public/%s/default' % source:
            instances.append(config.parent.parent)
    assert len(instances) == 1, '需要唯一的当前文件实例'
    instance = instances[0]
    command = ['docker', 'run', '--rm', '--network', 'host', '--read-only', '--user', '65532:65532',
               '--entrypoint', '/migration']
    for folder in ['grant', 'broker', 'database']:
        command += ['-v', '%s:/%s:ro' % (instance / folder, folder)]
    command += ['-v', '%s:/migration:ro' % (plugin / 'dist/file-migrate'),
                '-v', '%s:/legacy:ro' % (root / 'legacy')]
    migration_env = dict(env, AIO_PLUGIN_CONFIG='/grant/config.json', AIO_PLUGIN_SOCKET='/unused.sock',
                         AIO_FILE_LEGACY_DATABASE_URL=database, AIO_FILE_BINDINGS_DATABASE_URL=database,
                         AIO_FILE_LEGACY_STORAGE_DIR='/legacy')
    for name in ['AIO_PLUGIN_CONFIG', 'AIO_PLUGIN_SOCKET', 'AIO_FILE_LEGACY_DATABASE_URL',
                 'AIO_FILE_BINDINGS_DATABASE_URL', 'AIO_FILE_LEGACY_STORAGE_DIR']:
        command += ['-e', name]
    result = subprocess.check_output(command + [image], env=migration_env, universal_newlines=True)
    return json.loads(result)


try:
    assert sql("SELECT count(*) FROM pg_tables WHERE schemaname='public'") == '0', '请使用新的专用测试库'
    supervisor = subprocess.Popen([str(binary), 'supervisor'], cwd=str(root), env=env, stdout=supervisor_log, stderr=subprocess.STDOUT)
    until(lambda: (root / 'supervisor.sock').exists())
    launch()
    status, headers, _ = request('/api/auth/login', {'account': env['AIO_BOOTSTRAP_ACCOUNT'], 'password': env['AIO_BOOTSTRAP_PASSWORD']})
    assert status == 200
    cookie = headers['Set-Cookie'].split(';')[0]
    assert request('/api/files')[0] == 404
    first = publish('0.1.0')
    source = first['source_id']
    git = 'https://github.com/org-aio/aio-plugin-file.git'
    success('/api/runtime/plugins/install', {'git': git})
    token = mount(source, '0.1.0')
    assert invoke(token, 'GET', '/files')['max_file_bytes'] == 104857600
    # 上传过程中重启真实宿主和 Sidecar；已提交分块不可替换，剩余内容可续传。
    pending = invoke(token, 'POST', '/uploads', {'name': 'resumed.bin', 'content_type': 'application/octet-stream', 'size_bytes': 5 * 1024 * 1024})
    first_chunk = b'a' * pending['chunk_bytes']
    invoke(token, 'PUT', '/uploads/%s/chunks/0' % pending['id'], raw=first_chunk)
    stop(); launch()
    token = mount(source, '0.1.0')
    invoke(token, 'GET', '/uploads/' + pending['id'])
    invoke(token, 'PUT', '/uploads/%s/chunks/0' % pending['id'], raw=b'b' * pending['chunk_bytes'], expected=409)
    invoke(token, 'PUT', '/uploads/%s/chunks/0' % pending['id'], raw=first_chunk)
    last_chunk = b'c' * (1024 * 1024)
    invoke(token, 'PUT', '/uploads/%s/chunks/1' % pending['id'], raw=last_chunk)
    resumed = invoke(token, 'POST', '/uploads/%s/complete' % pending['id'])
    assert hashlib.sha256(download(token, resumed)[2]).hexdigest() == hashlib.sha256(first_chunk + last_chunk).hexdigest()
    large = upload(token, '100MiB.bin', 100 * 1024 * 1024)
    status, _, content = download(token, large)
    assert status == 200 and len(content) == large['size_bytes']
    assert hashlib.sha256(content).hexdigest() == large['sha256']
    del content
    empty = upload(token, 'empty.txt', 0, 'text/plain')
    assert download(token, empty)[2] == b''
    image_item = upload(token, 'stream.png', 5 * 1024 * 1024, 'image/png')
    public = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with public.open(base + image_item['public_path']) as response:
        assert hashlib.sha256(response.read()).hexdigest() == image_item['sha256']
    assert request(image_item['public_path'].replace('/default/', '/foreign/'))[0] == 404
    print('Verified 100 MiB through actual host, Docker and object broker', flush=True)

    legacy_id = str(uuid.uuid4()); legacy_token = uuid.uuid4().hex + uuid.uuid4().hex
    original = png + b'legacy-file-content'
    legacy = root / 'legacy'; legacy.mkdir(mode=0o755)
    bucket = legacy / hashlib.sha256(b'default').hexdigest(); bucket.mkdir(mode=0o755)
    (bucket / (legacy_id + '.blob')).write_bytes(original)
    sql("CREATE TABLE public.file_objects (tenant_id text,id text,original_name text,content_type text,size_bytes bigint,sha256 text,storage_name text,uploaded_by text,image_token text,created_at timestamptz)")
    sql("INSERT INTO public.file_objects VALUES ('default','%s','legacy.png','image/png',%s,'%s','%s.blob','legacy-author','%s','2026-01-02T03:04:05.123456Z')" % (legacy_id, len(original), hashlib.sha256(original).hexdigest(), legacy_id, legacy_token))
    assert migrate(source)['copied'] == 1
    again = migrate(source)
    assert again['copied'] == 0 and again['verified'] == 1
    imported = next(item for item in invoke(token, 'GET', '/files')['files'] if item['id'] == legacy_id)
    assert imported['uploaded_by'] == 'legacy-author' and imported['image_token'] == legacy_token
    assert download(token, imported)[2] == original
    assert (bucket / (legacy_id + '.blob')).read_bytes() == original
    assert sql('SELECT count(*) FROM public.file_objects') == '1'
    print('Verified migration against the actual runtime grant and broker', flush=True)

    browser = os.environ.get('AIO_FILE_DELIVERY_BROWSER_CONTAINER')
    if browser:
        browser_env = dict(env, AIO_URL=base, AIO_FILE_SOURCE=source)
        subprocess.check_call(['docker', 'exec', '-w', '/workspace', '-e', 'AIO_URL', '-e', 'AIO_FILE_SOURCE', '-e', 'AIO_BOOTSTRAP_ACCOUNT', '-e', 'AIO_BOOTSTRAP_PASSWORD', browser, 'node', 'tests/browser/files.cjs'], env=browser_env)

    second = publish('0.1.1')
    success('/api/runtime/plugins/install', {'git': git})
    token = mount(source, '0.1.1')
    assert download(token, large)[0] == 200
    success('/api/runtime/plugins/%s/rollback' % source, {})
    token = mount(source, '0.1.0')
    assert download(token, imported)[2] == original
    success('/api/runtime/plugins/%s/disable' % source, {})
    assert not any(p['id'] == 'component:%s:file-list' % source for p in pages())
    assert request(image_item['public_path'])[0] == 404
    assert download(token, large)[0] >= 400
    stop(); launch()
    assert not any(p['id'] == 'component:%s:file-list' % source for p in pages())
    success('/api/runtime/plugins/%s/enable' % source, {})
    token = mount(source, '0.1.0')
    assert download(token, imported)[2] == original
    assert sql("SELECT excluded_digest FROM component_installations WHERE source_id='%s' AND tenant_id='default'" % source) == second['revision']
    grants = sql("WITH removed AS (DELETE FROM role_permissions WHERE tenant_id='default' AND permission='file:manage' RETURNING *) SELECT json_agg(removed) FROM removed")
    assert json.loads(grants)
    assert not any(p['id'] == 'component:%s:file-list' % source for p in pages())
    assert download(token, large)[0] >= 400
    assert request(image_item['public_path'])[0] == 200, '公开图片不依赖用户权限'
    sql("INSERT INTO role_permissions SELECT * FROM json_populate_recordset(NULL::role_permissions, '%s'::json) ON CONFLICT DO NOTHING" % grants.replace("'", "''"))
    token = mount(source)
    invoke(token, 'DELETE', '/files/' + image_item['id'])
    assert request(image_item['public_path'])[0] == 404
    schema = sql("SELECT schema_name FROM aio_plugin_host.database_bindings WHERE source_id='%s' AND tenant_id='default'" % source)
    assert schema.startswith('p_') and schema.replace('_', '').isalnum()
    # 专用测试库中推进墓碑到期时间，随后由重启的真实清理任务完成物理回收。
    stop()
    sql("UPDATE \"%s\".file_objects SET cleanup_after=now() WHERE state='deleting'" % schema)
    launch()
    until(lambda: sql("SELECT count(*) FROM \"%s\".file_objects WHERE id='%s'" % (schema, image_item['id'])) == '0')
    assert hashlib.sha256(binary.read_bytes()).hexdigest() == host_hash
    success('/api/runtime/plugins/%s/uninstall' % source, {})
    assert not any(p['id'] == 'component:%s:file-list' % source for p in pages())
    report = {'largeFileBytes': 104857600, 'publicImages': True, 'migrationPreservedIdentity': True,
              'upgradeRollback': True, 'disabledAcrossRestart': True, 'liveRevocation': True,
              'cleanupAfterRestart': True, 'uploadResumedAfterRestart': True,
              'hostUnchanged': True, 'browser': bool(browser), 'logs': str(root)}
finally:
    try:
        stop()
    finally:
        if supervisor is not None and supervisor.poll() is None:
            supervisor.terminate(); supervisor.wait(timeout=20)
        host_log.close(); supervisor_log.close()

report['gracefulShutdown'] = True
print(json.dumps(report), flush=True)
