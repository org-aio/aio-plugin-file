const assert = require('node:assert/strict');
const {createReadStream} = require('node:fs');
const {mkdir, readFile, unlink, writeFile} = require('node:fs/promises');
const {createHash} = require('node:crypto');
const {basename, resolve} = require('node:path');
const {randomUUID} = require('node:crypto');
const {setTimeout: delay} = require('node:timers/promises');
const {chromium} = require('playwright');
const base = process.env.AIO_URL;
assert.equal(new URL(base).hostname, '127.0.0.1', '仅允许隔离本机宿主');
const output = resolve('test-results');
const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9Zl1sAAAAASUVORK5CYII=', 'base64');

async function until(check) {
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    if (await check()) return;
    await delay(50);
  }
  throw new Error('等待界面状态超时');
}

async function scenario(browser, mobile) {
  const context = await browser.newContext({viewport: mobile ? {width:390,height:844} : {width:1440,height:900}, isMobile:mobile, hasTouch:mobile, acceptDownloads:true});
  await context.grantPermissions(['clipboard-read','clipboard-write'], {origin:base});
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => {if(message.type()==='error') errors.push(message.text());});
  const prefix = `files-${mobile?'mobile':'desktop'}-${randomUUID().slice(0,8)}`;
  let frame;
  const rpc = (method,path,value) => frame.locator('body').evaluate((_,args)=>window.aioPlugin.json(...args), [method,path,value]);
  async function upload(name, buffer, mime, filePath) {
    await frame.getByRole('button',{name:'上传文件',exact:true}).first().click();
    const dialog = frame.getByRole('dialog',{name:'上传文件',exact:true});
    await dialog.getByLabel('选择文件',{exact:true}).setInputFiles(filePath ? filePath : {name,mimeType:mime,buffer});
    await dialog.getByRole('button',{name:'上传',exact:true}).click();
    await dialog.waitFor({state:'detached',timeout:180000});
    const listing = (await rpc('GET','/files')).data;
    const expectedName = filePath ? basename(filePath) : name;
    const created = listing.files.find(file => file.name === expectedName);
    assert(created,'上传完成后必须出现在文件列表：'+name);
    return created;
  }
  async function hashFile(path) {
    const digest = createHash('sha256');
    for await (const chunk of createReadStream(path)) digest.update(chunk);
    return digest.digest('hex');
  }
  async function search(value) {
    const input = frame.getByRole('searchbox',{name:'搜索文件',exact:true});
    await input.fill(value+'x');
    await input.press('End');
    await input.press('Backspace');
    assert.equal(await input.inputValue(),value);
    await input.press('Enter');
    assert.equal(await frame.getByRole('dialog').count(),0);
  }
  async function select(name,id) {
    let option = frame.getByRole('listbox',{name:'文件',exact:true}).getByRole('option').filter({hasText:name});
    if (id) option = frame.getByRole('listbox',{name:'文件',exact:true}).locator('[data-file-id="'+id+'"]');
    await option.first().locator('.file-browser__row-main').click();
  }
  try {
    const login=await context.request.post(base+'/api/auth/login',{data:{account:process.env.AIO_BOOTSTRAP_ACCOUNT,password:process.env.AIO_BOOTSTRAP_PASSWORD}});
    assert(login.ok());
    assert.equal((await context.request.get(base+'/api/files')).status(),404);
    await page.goto(base);
    await page.locator('.application-shell:visible').waitFor();
    await page.getByRole('navigation',{name:'场景'}).getByRole('button',{name:'系统',exact:true}).click();
    if(mobile) await page.getByRole('button',{name:'打开菜单',exact:true}).click();
    const nav=mobile?page.getByRole('dialog'):page.locator('.application-shell__sidebar');
    await nav.getByRole('button',{name:'文件列表',exact:true}).click();
    frame=page.frameLocator('iframe[title="文件列表"]');
    await frame.getByRole('heading',{name:'文件管理',exact:true}).waitFor();
    await frame.getByRole('searchbox',{name:'搜索文件',exact:true}).waitFor();
    const item=await upload(prefix+'.png',png,'image/png');
    await select(item.name);
    const preview=frame.getByRole('img',{name:item.name,exact:true});
    await preview.evaluate(image=>image.decode());
    assert(await preview.evaluate(image=>image.naturalWidth>0));
    const download=page.waitForEvent('download');
    await frame.getByRole('button',{name:'下载',exact:true}).click();
    assert.deepEqual(await readFile(await (await download).path()),png);
    await frame.getByRole('button',{name:'复制图床链接',exact:true}).click();
    await frame.getByText('已复制图床链接：'+item.name,{exact:true}).waitFor();
    assert((await page.evaluate(()=>navigator.clipboard.readText())).includes(item.public_path));

    if (!mobile) {
      const largePath = resolve(output,'100MiB.bin');
      await writeFile(largePath,Buffer.alloc(100 * 1024 * 1024, 37));
      const large = await upload(prefix+'-100MiB.bin',null,'application/octet-stream',largePath);
      await select(large.name,large.id);
      const downloaded = page.waitForEvent('download');
      await frame.getByRole('button',{name:'下载',exact:true}).click();
      const largeDownloadedPath = await (await downloaded).path();
      assert.equal(await hashFile(largeDownloadedPath), await hashFile(largePath));
      await unlink(largePath);
      await rpc('DELETE','/files/'+large.id);
      await frame.getByRole('button',{name:'刷新文件',exact:true}).click();
    }

    const initial=page.url();
    await search(prefix);
    await page.waitForFunction(before=>location.href!==before,initial);
    const shared=page.url();
    await page.goBack();
    await until(async()=>await frame.getByRole('searchbox',{name:'搜索文件',exact:true}).inputValue()==='');
    await page.goForward();
    await frame.getByRole('listbox',{name:'文件',exact:true}).getByText(item.name,{exact:true}).waitFor();
    assert.equal(await frame.getByRole('searchbox',{name:'搜索文件',exact:true}).inputValue(),prefix);
    await page.reload();
    await frame.getByRole('searchbox',{name:'搜索文件',exact:true}).waitFor();
    assert.equal(await frame.getByRole('searchbox',{name:'搜索文件',exact:true}).inputValue(),prefix);
    const fresh=await browser.newContext();
    try {
      assert((await fresh.request.post(base+'/api/auth/login',{data:{account:process.env.AIO_BOOTSTRAP_ACCOUNT,password:process.env.AIO_BOOTSTRAP_PASSWORD}})).ok());
      const reopened=await fresh.newPage();await reopened.goto(shared);
      const restored=reopened.frameLocator('iframe[title="文件列表"]');
      await restored.getByRole('searchbox',{name:'搜索文件',exact:true}).waitFor();
      assert.equal(await restored.getByRole('searchbox',{name:'搜索文件',exact:true}).inputValue(),prefix);
      await restored.getByRole('listbox',{name:'文件',exact:true}).getByText(item.name,{exact:true}).waitFor();
    } finally {await fresh.close();}

    await frame.getByRole('listbox',{name:'文件分类',exact:true}).getByRole('option').filter({hasText:'图片'}).click();
    await frame.getByRole('button',{name:'文件排序',exact:true}).click();
    await frame.getByRole('option',{name:'按名称',exact:true}).click();
    await until(async()=>await frame.getByRole('button',{name:'文件排序',exact:true}).getByText('按名称',{exact:true}).count()===1);
    await page.reload();
    await until(async()=>await frame.getByRole('button',{name:'文件排序',exact:true}).getByText('按名称',{exact:true}).count()===1);
    assert.equal(await frame.getByRole('listbox',{name:'文件分类',exact:true}).getByRole('option').filter({hasText:'图片'}).getAttribute('aria-selected'),'true');

    for(let index=0;index<32;index++) {
      const session=(await rpc('POST','/uploads',{name:prefix+'-scroll-'+String(index).padStart(2,'0')+'.txt',content_type:'text/plain',size_bytes:0})).data;
      await rpc('POST','/uploads/'+session.id+'/complete');
    }
    await frame.getByRole('listbox',{name:'文件分类',exact:true}).getByRole('option').filter({hasText:'所有文件'}).click();
    await frame.getByRole('button',{name:'刷新文件',exact:true}).click();
    const listing=frame.getByRole('listbox',{name:'文件',exact:true});
    await until(async()=>await listing.getByRole('option').count()===33);
    const beforeScroll=page.url();
    const offset=await listing.evaluate(element=>{element.scrollTop=600;return element.scrollTop;});
    assert(offset>100,'文件列表必须能独立滚动');
    await until(async()=>page.url()!==beforeScroll);
    const scrollUrl=page.url();
    await page.reload();
    await until(async()=>await listing.evaluate(element=>element.scrollTop)>100);
    const sharedScroll=await browser.newContext({viewport:mobile?{width:390,height:844}:{width:1440,height:900}});
    try {
      assert((await sharedScroll.request.post(base+'/api/auth/login',{data:{account:process.env.AIO_BOOTSTRAP_ACCOUNT,password:process.env.AIO_BOOTSTRAP_PASSWORD}})).ok());
      const reopened=await sharedScroll.newPage();await reopened.goto(scrollUrl);
      const restored=reopened.frameLocator('iframe[title="文件列表"]').getByRole('listbox',{name:'文件',exact:true});
      await until(async()=>await restored.evaluate(element=>element.scrollTop)>100);
    } finally {await sharedScroll.close();}
    await frame.getByRole('listbox',{name:'文件分类',exact:true}).getByRole('option').filter({hasText:'图片'}).click();
    await frame.getByRole('button',{name:'刷新文件',exact:true}).click();
    await until(async()=>await listing.getByRole('option').count()===1);

    // 延迟一块真实请求，让用户取消；最终仍由真实服务确认会话已撤销。
    let release, entered;
    const gate=new Promise(resolve=>release=resolve);const observed=new Promise(resolve=>entered=resolve);
    let uploadId;
    await page.route('**/api/runtime/components/*/request',async route=>{
      const input=route.request().postDataJSON();
      if(input.method==='PUT' && input.path.includes('/chunks/')){uploadId=input.path.split('/')[2];entered();await gate;}
      await route.continue();
    });
    await frame.getByRole('button',{name:'上传文件',exact:true}).first().click();
    const dialog=frame.getByRole('dialog',{name:'上传文件',exact:true});
    await dialog.getByLabel('选择文件',{exact:true}).setInputFiles({name:prefix+'-cancel.bin',mimeType:'application/octet-stream',buffer:Buffer.alloc(5*1024*1024,7)});
    await dialog.getByRole('button',{name:'上传',exact:true}).click();
    let timer;
    try {await Promise.race([observed,new Promise((_,reject)=>{timer=setTimeout(()=>reject(new Error('等待分块上传超时')),30000);})]);}
    finally {clearTimeout(timer);}
    await dialog.getByRole('button',{name:'取消',exact:true}).click();release();
    await dialog.getByText('上传已取消',{exact:true}).waitFor();
    await page.unroute('**/api/runtime/components/*/request');
    const cancelled=await frame.locator('body').evaluate(async(_,id)=>{try{await window.aioPlugin.json('GET','/uploads/'+id);return false;}catch{return true;}},uploadId);
    assert(cancelled);
    assert(!(await rpc('GET','/files')).data.files.some(file=>file.name===prefix+'-cancel.bin'));
    await dialog.getByRole('button',{name:'取消',exact:true}).click();

    await select(item.name);
    await frame.getByRole('button',{name:'完整信息',exact:true}).click();
    const details=frame.getByRole('dialog',{name:'文件详情',exact:true});
    await details.getByText(item.sha256,{exact:true}).waitFor();
    await details.getByRole('button',{name:'关闭',exact:true}).click();
    await frame.locator('.admin-page').evaluate(async element=>{await document.fonts.ready;if(getComputedStyle(element).display!=='flex')throw new Error('共享样式未加载');});
    assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'宿主页横向溢出');
    assert(await frame.locator('body').evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'文件页横向溢出');
    await page.screenshot({path:resolve(output,(mobile?'mobile':'desktop')+'-files.png'),fullPage:true});
    await frame.getByRole('button',{name:'删除 '+item.name,exact:true}).click();
    const deletion=frame.getByRole('dialog',{name:'删除文件',exact:true});
    await deletion.getByRole('button',{name:'取消',exact:true}).click();
    assert((await context.request.get(base+item.public_path)).ok());
    await frame.getByRole('button',{name:'删除 '+item.name,exact:true}).click();
    await deletion.getByRole('button',{name:'确认删除',exact:true}).click();
    await deletion.waitFor({state:'detached'});
    assert.equal((await context.request.get(base+item.public_path)).status(),404);
    assert.deepEqual(errors,[]);
    return {mobile,upload:true,largeFileBytes:mobile?0:104857600,cancel:true,download:true,publicImage:true,clipboard:true,sharedUrl:true,sharedScroll:true,history:true,categorySort:true,deleteConfirmation:true,overflow:false};
  } catch(error) {
    await page.screenshot({path:resolve(output,(mobile?'mobile':'desktop')+'-failure.png'),fullPage:true});throw error;
  } finally {await context.close();}
}
(async()=>{
  await mkdir(output,{recursive:true});const browser=await chromium.launch({headless:true});
  try {const results=[await scenario(browser,false),await scenario(browser,true)];await writeFile(resolve(output,'report.json'),JSON.stringify(results,null,2));console.log(JSON.stringify(results));}
  finally {await browser.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
