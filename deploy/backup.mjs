import {DatabaseSync,backup} from 'node:sqlite';
import {mkdirSync,readdirSync,unlinkSync} from 'node:fs';
const directory='/data/backups';mkdirSync(directory,{recursive:true});
const db=new DatabaseSync('/data/poker.db');
const file=`poker-${new Date().toISOString().replaceAll(':','-')}.db`;
await backup(db,`${directory}/${file}`);db.close();
for(const old of readdirSync(directory).filter(f=>/^poker-.*\.db$/.test(f)).sort().slice(0,-14))unlinkSync(`${directory}/${old}`);
console.log(`Backed up ${file}; kept latest 14 backups`);
