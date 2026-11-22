// test 用の module。読み込んでも走らない。citty は使わず、main の形だけ合わせた object を出す。
export const main = { meta: { name: 'box-cli' } };

export async function run(rawArgs) {
  const [sub, ...rest] = rawArgs;
  if (sub === 'echo') {
    console.log(JSON.stringify({ via: 'module', argv: rawArgs }));
    return 0;
  }
  if (sub === 'bad') {
    console.error('usage error');
    return 2;
  }
  if (sub === 'fail') {
    console.error('failed');
    return 1;
  }
  console.error(`unknown: ${sub} ${rest.join(' ')}`);
  return 2;
}
