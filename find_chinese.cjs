const fs = require('fs');
const path = require('path');
function walk(dir) {
  let results = [];
  const list = fs.readdirSync(dir);
  list.forEach(file => {
    file = path.join(dir, file);
    const stat = fs.statSync(file);
    if (stat && stat.isDirectory()) { 
      results = results.concat(walk(file));
    } else if (file.endsWith('.vue')) {
      results.push(file);
    }
  });
  return results;
}
const files = walk('./src/components').concat(walk('./src/views'));
files.forEach(f => {
  const lines = fs.readFileSync(f, 'utf8').split('\n');
  lines.forEach((line, i) => {
    if (/[\u4e00-\u9fa5]/.test(line) && !line.trim().startsWith('//') && !line.trim().startsWith('<!--') && !line.trim().startsWith('*')) {
      console.log(`${f}:${i+1}: ${line.trim()}`);
    }
  });
});
