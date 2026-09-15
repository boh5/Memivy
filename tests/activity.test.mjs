import test from 'node:test';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {streaks, calendarDays, dayKey} from '../src/workspace/activityCalendar.ts';
import {workspaceFixture} from './helpers/workspace.mjs';

const entries = dates => dates.map(date => ({date,count:1}));
test('annual grids reserve full-year columns even on the first day of the year',()=>{
 for (const year of ['2026','2028']) {
  const january=calendarDays(year,`${year}-01-01`);
  const december=calendarDays(year,`${year}-12-31`);
  assert.equal(january.length,december.length,'Calendar width must not depend on elapsed weeks');
  assert(january.length/7>=52,'A partial first week must not stretch across the full chart');
  assert.deepEqual(january.filter(cell=>cell.visible).map(cell=>cell.date),[`${year}-01-01`]);
  assert.equal(calendarDays(year,`${year}-01-15`).filter(cell=>cell.visible).length,15);
  assert.equal(january.find(cell=>cell.date===`${year}-12-31`).visible,false);
 }
});
test('calendar handles leap years and streaks at midnight and across years',()=>{
 assert.equal(calendarDays('2024','2026-09-15').filter(cell=>cell.visible).length,366);
 assert.equal(calendarDays('recent','2026-09-15').filter(cell=>cell.visible).length,365);
 assert(calendarDays('2026','2026-09-15').every(cell=>!cell.visible||cell.date<='2026-09-15'));
 assert.deepEqual(streaks(entries(['2023-12-30','2023-12-31','2024-01-01']),'2024-01-02'),{current:3,longest:3});
 assert.deepEqual(streaks(entries(['2023-12-30','2023-12-31','2024-01-01']),'2024-01-03'),{current:0,longest:3});
 assert.deepEqual(streaks(entries(['2024-02-28','2024-02-29','2024-03-01']),'2024-03-01'),{current:3,longest:3});
 assert.deepEqual(streaks([],'2026-09-15'),{current:0,longest:0});
});
test('local-day ranges follow daylight saving instead of fixed 24-hour windows',()=>{
 const result=spawnSync(process.execPath,['--experimental-strip-types','--input-type=module','-e',`import {dayBounds,moveDay} from './src/workspace/activityCalendar.ts'; console.log(JSON.stringify(['2026-03-08','2026-11-01'].map(date=>{const b=dayBounds(date);return [(b.until-b.since)/3600000,moveDay(date,1)];})));`],{encoding:'utf8',env:{...process.env,TZ:'America/Los_Angeles'}});
 assert.equal(result.status,0,result.stderr);
 assert.deepEqual(JSON.parse(result.stdout),[[23,'2026-03-09'],[25,'2026-11-02']]);
});
test('activity selects a day, ignores late responses, and opens the existing detail',async t=>{
 const f=workspaceFixture(t,{timers:{setTimeout,clearTimeout,setInterval:()=>0,clearInterval(){},ResizeObserver:class{observe(){} disconnect(){}}}});
 const Activity=f.load('src/workspace/Activity.tsx').default;
 const today=dayKey(new Date()),yesterday=new Date();yesterday.setDate(yesterday.getDate()-1);
 const prior=dayKey(yesterday);let resolveOld,opened;
 f.overrides.activity_summary=async()=>({memory_count:2,days:entries([prior,today])});
 f.overrides.activity_records=({since})=> since===new Date(`${prior}T00:00:00`).getTime()?new Promise(resolve=>{resolveOld=resolve;}):Promise.resolve({items:[{id:'record-today',key:f.keyA,title:'Today original',origin:null,updated_at:Date.now()}],next_offset:null});
 const view=f.mount(Activity,{onOpenRecord:key=>{opened=key;}});await f.settle();
 assert.equal(f.calls.filter(call=>call.name==='activity_records').length,0);
 f.find(view,n=>n.props['data-date']===prior).props.onClick();await f.settle();
 f.find(view,n=>n.props['data-date']===today).props.onClick();await f.settle();
 resolveOld({items:[{id:'old',key:f.keyB,title:'Late old result'}],next_offset:null});await f.settle();
 assert(f.text(view.tree).includes('Today original'));assert(!f.text(view.tree).includes('Late old result'));
 f.find(view,n=>n.props.className==='activity-record').props.onClick();assert.deepEqual(opened,f.keyA);
 const focusable=f.nodes(view.tree).filter(n=>n.props['data-date']&&n.props.tabIndex===0);assert.equal(focusable.length,1);
});
