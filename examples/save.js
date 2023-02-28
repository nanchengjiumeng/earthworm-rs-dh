const { Screenshot } = require("..");
const fs = require("fs");

const ss = new Screenshot(
  "001 - VMware Workstation",
  "MKSWindow#0",
  "D:\\shadowUTF8.lib"
);
// const ss = new Screenshot("新建文本文档 (2).txt - 记事本");
// const ss = new Screenshot();

// const start = Date.now();
// let buf = ss.takeBmp(0, 0, 500, 500);
// const list = ss.dhOcrShadowText(0, 0, 1024, 768, 100);
// console.log(Date.now() - start);
// console.log(list);
// fs.writeFileSync("test.bmp", buf);
// buf = null;
// }, 1000);

async function wait(ms = 3000) {
  return new Promise((r) => {
    setTimeout(() => {
      r();
    }, ms);
  });
}

async function test_same_pro(count = 3, maxCount = 3) {
  if (count > 0) {
    let buf = ss.takeBmp(0, 0, 500, 500);
    let c = maxCount + 1 - count;
    fs.writeFileSync(`test_${c}.bmp`, buf);
    console.log(c);
    if (count - 1 > 0) {
      await wait();
    }
    return test_same_pro(count - 1, maxCount);
  }
}

test_same_pro();
