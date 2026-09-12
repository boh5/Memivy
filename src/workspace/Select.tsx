import { Children, isValidElement, useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { Icon } from '../ui';
import './select.css';

/** A native top-layer popover keeps options visible inside scrolling dialogs. */
export default function Select({value, onChange, children, disabled=false, 'aria-label':label}: {
  value:string; onChange:(event:{target:{value:string}})=>void; children:ReactNode; disabled?:boolean; 'aria-label':string;
}) {
  const id=useId(), trigger=useRef<HTMLButtonElement>(null), menu=useRef<HTMLDivElement>(null);
  const [open,setOpen]=useState(false), [active,setActive]=useState(0);
  const search=useRef({text:'',time:0});
  useEffect(()=>{
    if(!open)return;
    const dismiss=(event:Event)=>{if(event.target instanceof Node&&menu.current?.contains(event.target))return;menu.current?.hidePopover();setOpen(false);};
    window.addEventListener('resize',dismiss);window.addEventListener('scroll',dismiss,true);
    return()=>{window.removeEventListener('resize',dismiss);window.removeEventListener('scroll',dismiss,true);};
  },[open]);
  const options=Children.toArray(children).filter(isValidElement).map(child=>{
    const props=child.props as {value?:string;children:ReactNode};
    return {value:props.value??String(props.children),label:String(props.children)};
  });
  function close(){menu.current?.hidePopover();setOpen(false);}
  function show(){
    if(disabled||!options.length)return;
    const button=trigger.current, popup=menu.current;
    if(!button||!popup)return;
    button.focus({preventScroll:true});
    const r=button.getBoundingClientRect(),below=window.innerHeight-r.bottom-12,above=r.top-12;
    const height=Math.min(280,Math.max(below,above)), width=Math.min(Math.max(r.width,180),window.innerWidth-24);
    Object.assign(popup.style,{left:`${Math.max(12,Math.min(r.left,window.innerWidth-width-12))}px`,width:`${width}px`,maxHeight:`${height}px`,top:below>=Math.min(280,above)?`${r.bottom+5}px`:'auto',bottom:below>=Math.min(280,above)?'auto':`${window.innerHeight-r.top+5}px`});
    setActive(Math.max(0,options.findIndex(option=>option.value===value)));
    popup.showPopover();setOpen(true);
  }
  function choose(index:number){const option=options[index];if(!option)return;close();trigger.current?.focus();onChange({target:{value:option.value}});}
  function move(index:number){const next=(index+options.length)%options.length;setActive(next);menu.current?.children[next]?.scrollIntoView({block:'nearest'});}
  return <span className="workspace-select">
    <button type="button" role="combobox" ref={trigger} className="workspace-select-trigger" disabled={disabled} aria-label={label} aria-haspopup="listbox" aria-expanded={open} aria-controls={id} aria-activedescendant={open?`${id}-${active}`:undefined}
      onClick={()=>open?close():show()} onKeyDown={event=>{
        if(event.nativeEvent.isComposing)return;
        if(event.key==='Escape'&&open){event.preventDefault();event.stopPropagation();close();return;}
        if(event.key==='Tab'){close();return;}
        if(['ArrowDown','ArrowUp','Home','End'].includes(event.key)){
          event.preventDefault();if(!open){show();return;}
          move(event.key==='Home'?0:event.key==='End'?options.length-1:active+(event.key==='ArrowDown'?1:-1));return;
        }
        if((event.key==='Enter'||event.key===' ')&&open){event.preventDefault();choose(active);return;}
        if(event.key.length===1&&!event.metaKey&&!event.ctrlKey&&!event.altKey){
          event.preventDefault();const now=Date.now();search.current={text:(now-search.current.time<700?search.current.text:'')+event.key.toLocaleLowerCase(),time:now};
          const index=options.findIndex(option=>option.label.toLocaleLowerCase().startsWith(search.current.text));
          if(!open)show();if(index>=0)move(index);
        }
      }} onBlur={()=>close()}>
      <span>{options.find(option=>option.value===value)?.label??label}</span><Icon name="chevron" size={14}/>
    </button>
    <div id={id} ref={menu} popover="auto" className="workspace-select-menu" role="listbox" aria-label={label} onToggle={event=>setOpen(event.newState==='open')}>
      {options.map((option,index)=><div id={`${id}-${index}`} key={option.value} role="option" aria-selected={option.value===value} className={active===index?'highlighted':''}
        onPointerDown={event=>event.preventDefault()} onPointerMove={()=>setActive(index)} onClick={event=>{event.preventDefault();event.stopPropagation();choose(index);}}>
        <span>{option.label}</span>{option.value===value&&<span aria-hidden="true">✓</span>}
      </div>)}
    </div>
  </span>;
}
