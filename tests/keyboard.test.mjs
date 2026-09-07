import test from 'node:test';
import assert from 'node:assert/strict';
import { isSubmitKey } from '../src/workspace/keyboard.ts';
const enter = {key:'Enter', metaKey:false, ctrlKey:false, altKey:false, shiftKey:false, repeat:false};
test('Enter inserts a line and only an intentional Command Enter submits', () => {
  assert.equal(isSubmitKey(enter),false);
  assert.equal(isSubmitKey({...enter,shiftKey:true}),false);
  const submit={...enter,metaKey:true};
  assert.equal(isSubmitKey(submit),true);
  for(const patch of [{isComposing:true},{keyCode:229},{repeat:true},{ctrlKey:true},{altKey:true},{shiftKey:true}]) assert.equal(isSubmitKey({...submit,...patch}),false);
  assert.equal(isSubmitKey(submit,true),false);
});
