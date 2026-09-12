import test from 'node:test';
import assert from 'node:assert/strict';
import {createServer} from 'vite';
import {createElement} from 'react';
import {renderToStaticMarkup} from 'react-dom/server';

test('Markdown renders structure and search highlights without executing HTML or loading images',async()=>{
  const server=await createServer({server:{middlewareMode:true,watch:null},logLevel:'silent'});
  try {
    const {default:i18n}=await server.ssrLoadModule('/src/i18n/index.ts');
    const {initReactI18next}=await import('react-i18next');
    await i18n.use(initReactI18next).init({lng:'zh-CN',resources:{'zh-CN':{editor:JSON.parse(await (await import('node:fs/promises')).readFile(new URL('../locales/zh-CN/editor.json',import.meta.url),'utf8'))}}});
    const {default:Markdown}=await server.ssrLoadModule('/src/workspace/Markdown.tsx');
    const html=renderToStaticMarkup(createElement(Markdown,{query:'中文',text:'## 中文标题\n\n**中文强调**\n\n> 引用\n\n- 列表\n\n```js\nconst n = 1;\n```\n\n| 甲 | 乙 |\n| --- | --- |\n| 1 | 2 |\n\n![外部图片](https://example.com/tracker.png)\n\n[危险链接](javascript:alert(1))\n\n<script>alert(1)</script>\n\n[资料](https://example.com/)'}));
    assert.match(html,/<h2><mark>中文<\/mark>标题<\/h2>/);
    assert.match(html,/<blockquote>/);assert.match(html,/<ul>/);assert.match(html,/<pre><code/);assert.match(html,/<table>/);
    assert.match(html,/<strong><mark>中文<\/mark>强调<\/strong>/);
    assert(!html.includes('<script'));assert(!html.includes('<img'));assert(!html.includes('javascript:'));
    assert.match(html,/rel="noreferrer noopener"/);
    const underlined=renderToStaticMarkup(createElement(Markdown,{text:'<u>下划线与 **加粗**</u>\n\n- [x] 已完成\n- [ ] 未完成\n\n<u onclick="alert(1)">无效属性</u>'}));
    assert.match(underlined,/<u>下划线与 <strong>加粗<\/strong><\/u>/);
    assert.match(underlined,/class="task-list-item"/);assert.match(underlined,/checked=""/);
    assert(!underlined.includes('onclick'));
    const {unified}=await import('unified');
    const {default:parse}=await import('remark-parse');const {default:stringify}=await import('remark-stringify');
    const {remarkUnderline}=await server.ssrLoadModule('/src/workspace/remarkUnderline.ts');
    const processor=unified().use(parse).use(remarkUnderline).use(stringify);
    const input='<u>下划线与 **加粗**</u> 和 `代码`';
    const first=String(await processor.process(input));
    assert.match(first,/<u>下划线与 \*\*加粗\*\*<\/u>/);
    assert.equal(String(await processor.process(first)),first);
  } finally {await server.close();}
});
