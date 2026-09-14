// Fictional, fixed content for the clickable visual demo. Nothing is persisted.
window.MEMIVY_SEED = [
  {
    id: 'first-experience',
    title: 'First-use principles',
    project: 'Memivy',
    tone: 'yellow',
    updated: 'Today 14:32',
    excerpt: 'Show a useful result before asking people to make the space their own.',
    keywords: ['first use', 'show results first', 'configuration', 'new user', 'classification', 'getting started'],
    status: 'ready',
    captures: [
      {
        id: 'first-1',
        text: 'When I open a new tool, I want to see what it can keep for me. Let me write one sentence and see it saved before learning the other features.',
        app: 'Quick note',
        project: 'Memivy',
        time: '8 / 18   20:16',
        sourceLabel: 'A quick note'
      },
      {
        id: 'first-2',
        text: 'Perhaps a new AI product should show a useful result before asking for configuration. Looking at Bear reminded me that an empty page can feel inviting, provided the next step is clear.',
        app: 'Safari',
        project: 'Memivy',
        time: '9 / 3   10:24',
        sourceLabel: 'Bear · Attached webpage',
        url: 'https://bear.app/'
      },
      {
        id: 'first-3',
        text: 'Remember this conclusion: the main friction is being forced to classify an idea before it has formed. First use should save one original sentence and show where it went. Explain configuration when people need AI help.',
        app: 'Codex',
        project: 'Memivy',
        time: 'Today 14:32',
        sourceLabel: 'Sample conversation'
      }
    ],
    versions: [
      {
        number: 1,
        author: 'Memory Agent',
        time: '8 / 18   20:16',
        body: 'When people first open Memivy, give them a place to write. Seeing one sentence safely saved explains the product better than a screen full of features.\n\nThe rest can wait. Even an unfinished thought deserves somewhere to stay.',
        sourceIds: ['first-1']
      },
      {
        number: 2,
        author: 'Memory Agent',
        time: '9 / 3   10:24',
        body: 'Show a useful result before asking people to make the space their own. When first opened, Memivy should let people write a sentence and see it saved. A clearly labeled sample can show what organization looks like.\n\nAn empty page can be quiet and inviting, but the next step must be clear. Model configuration should wait until people want AI help, when we can explain what to enter.',
        sourceIds: ['first-1', 'first-2']
      },
      {
        number: 3,
        author: 'Memory Agent',
        time: 'Today 14:32',
        body: 'Show a useful result before asking people to make the space their own. When people first open Memivy, one sentence produces a visible saved note; sample memories show organized content.\n\nDo not rush unfinished ideas into titles and folders. Save them, find connections, and show where they went.\n\nDecisions can be corrected and original notes stay intact. Configure models when AI is needed. Until then, capture and keyword search remain available.',
        sourceIds: ['first-1', 'first-2', 'first-3']
      }
    ]
  },
  {
    id: 'walking',
    title: 'Why I walk',
    project: 'Everyday',
    tone: 'teal',
    updated: 'Today 08:46',
    excerpt: 'Some questions become easier when I stop sitting and start walking.',
    keywords: ['walking', 'space', 'riverside', 'thinking', 'rest', 'rhythm'],
    status: 'ready',
    captures: [
      {
        id: 'walking-1',
        text: 'I walked by the river for forty minutes after dinner without a podcast. By the second bridge I noticed the lights on the water. Not every minute needs new input.',
        app: 'Quick note',
        project: 'Everyday',
        time: '9 / 2   21:08',
        sourceLabel: 'A note after getting home'
      },
      {
        id: 'walking-2',
        text: 'I walked that route again this morning. Shops were opening and the florist was changing the water. Nothing clicked suddenly, but I knew I could set the previous plan aside. A walk does not need to produce anything.',
        app: 'Quick note',
        project: 'Everyday',
        time: 'Today 08:46',
        sourceLabel: 'A quick note'
      }
    ],
    versions: [
      {
        number: 1,
        author: 'Memory Agent',
        time: '9 / 2   21:08',
        body: 'I left my headphones at home and walked by the river. Thoughts about the page gave way to bridges and lights. Quiet time lets what I have already heard settle.',
        sourceIds: ['walking-1']
      },
      {
        number: 2,
        author: 'Me',
        time: 'Today 08:46',
        body: 'The forty-minute walk did not produce a perfect answer. At the second bridge the competing plans grew quiet, and I started noticing lights stretching across the water as boats passed.\n\nI used to fill every walk with headphones. After too much input, another voice rarely makes things clearer. Silence gives unfinished thoughts room to settle.\n\nThe same route feels different in the morning and evening. Shutters rise, flowers change, and chairs appear outside shops. The city keeps its own pace while I consider where to put a button.\n\nBack home, I realized I could set the plan aside. It did not finish the work for me, but it helped me relax. Sometimes judgment needs distance from the screen rather than one more argument.\n\nI do not want walking to become another productivity tool. A short loop, a bag of oranges, and a pleasant breeze can be enough. Going outside needs no elaborate reason.\n\nWhen my thoughts get noisy, I can take a walk without a step target or a problem to solve. A familiar route helps me notice what is in front of me.',
        sourceIds: ['walking-1', 'walking-2']
      }
    ]
  },
  {
    id: 'memory-boundaries',
    title: 'AI memory boundaries',
    project: 'Memivy',
    tone: 'rose',
    updated: 'Yesterday 17:20',
    excerpt: 'Being remembered should be a choice. What matters is being able to return to the original words.',
    keywords: ['AI', 'Memories', 'original notes', 'sources', 'Undo', 'trust', 'boundaries'],
    status: 'ready',
    captures: [
      {
        id: 'boundary-1',
        text: 'I do not want a tool to decide which moments deserve saving. It should wait until I say “remember this.” More context does not always mean better understanding.',
        app: 'Quick note',
        project: 'Memivy',
        time: '9 / 1   22:11',
        sourceLabel: 'A quick note'
      },
      {
        id: 'boundary-2',
        text: 'Remember our agreed boundaries: organization can create a new current version, but original notes stay intact. Each update should show its sources and support undo. Trust comes from being able to check.',
        app: 'Claude Code',
        project: 'Memivy',
        time: 'Yesterday 17:20',
        sourceLabel: 'Sample conversation'
      }
    ],
    versions: [
      {
        number: 1,
        author: 'Memory Agent',
        time: '9 / 1   22:11',
        body: 'Being remembered should be a choice. When I say “remember this,” the tool can save it. Respecting what I choose matters more than collecting everything.',
        sourceIds: ['boundary-1']
      },
      {
        number: 2,
        author: 'Memory Agent',
        time: 'Yesterday 17:20',
        body: 'Being remembered should be a choice. When I say “remember this,” the tool can save it. Respecting what I choose matters more than collecting everything.\n\nAI can connect scattered notes without replacing the original words. The organized version can grow while the original remains available.\n\nEach update should explain what changed and which sources it used, and let me change my mind. Trust comes from evidence, not confidence alone.',
        sourceIds: ['boundary-1', 'boundary-2']
      }
    ]
  },
  {
    id: 'city-weekend',
    title: 'A weekend in the city',
    project: 'Everyday',
    tone: 'coral',
    updated: '9 / 3  ',
    excerpt: 'Choose a starting point and leave the rest to the streets and weather.',
    keywords: ['weekend', 'city', 'wandering', 'bookshop', 'streets', 'travel'],
    status: 'ready',
    captures: [
      {
        id: 'city-1',
        text: 'The best part of last weekend was not on a saved list. A wrong turn after the bookshop led to a quiet street and a tiny noodle shop. Next time I will choose only a starting point.',
        app: 'Quick note',
        project: 'Everyday',
        time: '9 / 3   19:05',
        sourceLabel: 'A quick note'
      }
    ],
    versions: [
      {
        number: 1,
        author: 'Memory Agent',
        time: '9 / 3   19:05',
        body: 'A wrong turn after the bookshop led to a quiet street and a two-table noodle shop. That unplanned stop became my favorite part of the afternoon.\n\nStart with a bookshop worth visiting. Leave room to sit down or follow an interesting doorway. Getting to know a city sometimes means leaving space for chance.',
        sourceIds: ['city-1']
      }
    ]
  },
  {
    id: 'writing-voice',
    title: 'Keep the voice in a first draft',
    project: 'Writing',
    tone: 'yellow',
    updated: '9 / 2  ',
    excerpt: 'Write the sentence that belongs to this moment. Polish it on the second pass.',
    keywords: ['Writing', 'voice', 'first draft', 'expression', 'revision', 'inspiration'],
    status: 'ready',
    captures: [
      {
        id: 'writing-1',
        text: 'I rewrote the opening three times until it sounded correct but no longer sounded like me. The first pass should let the words out before smoothing every edge.',
        app: 'Quick note',
        project: 'Writing',
        time: '9 / 2   23:14',
        sourceLabel: 'A thought while drafting'
      }
    ],
    versions: [
      {
        number: 1,
        author: 'Memory Agent',
        time: '9 / 2   23:14',
        body: 'Write the sentence that belongs to this moment. A polished opening can be correct while losing its voice.\n\nKeep the uncertain judgment, unusual comparison, or unfinished sentence in the first draft. Once the idea stands, refine the wording without sanding away every edge.',
        sourceIds: ['writing-1']
      }
    ]
  },
  {
    id: 'coffee',
    title: 'Coffee flavors',
    project: 'Everyday',
    tone: 'teal',
    updated: '9 / 1  ',
    excerpt: 'As it cools, I can taste a little apricot-like acidity.',
    keywords: ['coffee', 'flavor', 'apricot', 'pour-over', 'temperature'],
    status: 'ready',
    captures: [
      {
        id: 'coffee-1',
        text: 'Freshly brewed, this coffee mainly smelled good. As it cooled, I noticed apricot-like acidity. I tend to judge the first sip too quickly.',
        app: 'Quick note',
        project: 'Everyday',
        time: '9 / 1   09:27',
        sourceLabel: 'A breakfast note'
      }
    ],
    versions: [
      {
        number: 1,
        author: 'Memory Agent',
        time: '9 / 1   09:27',
        body: 'I enjoyed this coffee more after it cooled. The aroma came first; then a little apricot-like acidity emerged.\n\nDo not rush to judge the first sip. The same cup can taste different a few minutes later.',
        sourceIds: ['coffee-1']
      }
    ]
  },
  {
    id: 'unfinished-thought',
    title: 'An unfinished thought',
    project: 'Unassigned',
    tone: 'rose',
    updated: '8 / 31  ',
    excerpt: 'Perhaps some things do not need to be useful immediately.',
    keywords: ['thought', 'useful', 'Unassigned'],
    status: 'unassigned',
    captures: [
      {
        id: 'unfinished-1',
        text: 'Perhaps some things do not need to be useful immediately.',
        app: 'Quick note',
        project: 'Unassigned',
        time: '8 / 31   22:38',
        sourceLabel: 'A quick note'
      }
    ],
    versions: []
  }
];

window.MEMIVY_EXAMPLES = {
  append: 'One more first-use thought: after writing the first sentence, people should immediately see “Saved.” Model configuration can wait. Do not make a new thought wait for setup.',
  new: 'Copying a favorite sentence by hand feels different from bookmarking it. Halfway through, I began to hear its rhythm.',
  uncertain: 'Perhaps we can leave a little more space.'
};
