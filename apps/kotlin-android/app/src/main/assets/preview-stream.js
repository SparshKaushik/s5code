var T3PreviewStream = (function (e) {
  Object.defineProperty(e, Symbol.toStringTag, { value: `Module` });
  var t = (e, t) => {
      let n = Object.entries(t.query);
      return n.length === 0
        ? e
        : `${e}${e.includes(`?`) ? `&` : `?`}${new URLSearchParams(n).toString()}`;
    },
    n = (e, t) => {
      switch (t.length) {
        case 0:
          return e;
        case 1:
          return t[0](e);
        case 2:
          return t[1](t[0](e));
        case 3:
          return t[2](t[1](t[0](e)));
        case 4:
          return t[3](t[2](t[1](t[0](e))));
        case 5:
          return t[4](t[3](t[2](t[1](t[0](e)))));
        case 6:
          return t[5](t[4](t[3](t[2](t[1](t[0](e))))));
        case 7:
          return t[6](t[5](t[4](t[3](t[2](t[1](t[0](e)))))));
        case 8:
          return t[7](t[6](t[5](t[4](t[3](t[2](t[1](t[0](e))))))));
        case 9:
          return t[8](t[7](t[6](t[5](t[4](t[3](t[2](t[1](t[0](e)))))))));
        default: {
          let n = e;
          for (let e = 0, r = t.length; e < r; e++) n = t[e](n);
          return n;
        }
      }
    },
    r = {
      pipe() {
        return n(this, arguments);
      },
    },
    i = (function () {
      function e() {}
      return ((e.prototype = r), e);
    })(),
    a = function (e, t) {
      if (typeof e == `function`)
        return function () {
          return e(arguments) ? t.apply(this, arguments) : (e) => t(e, ...arguments);
        };
      switch (e) {
        case 0:
        case 1:
          throw RangeError(`Invalid arity ${e}`);
        case 2:
          return function (e, n) {
            return arguments.length >= 2
              ? t(e, n)
              : function (n) {
                  return t(n, e);
                };
          };
        case 3:
          return function (e, n, r) {
            return arguments.length >= 3
              ? t(e, n, r)
              : function (r) {
                  return t(r, e, n);
                };
          };
        default:
          return function () {
            if (arguments.length >= e) return t.apply(this, arguments);
            let n = arguments;
            return function (e) {
              return t(e, ...n);
            };
          };
      }
    },
    o = (e) => e,
    s = (e) => () => e,
    c = s(!0),
    l = s(null),
    u = s(void 0),
    d = u;
  function f(e) {
    let t = new WeakMap();
    return (n) => {
      let r = t.get(n);
      if (r !== void 0) return r;
      let i = e(n);
      return (t.set(n, i), i);
    };
  }
  function ee(e) {
    let t = new WeakMap();
    return (n) => {
      let r = t.get(n);
      if (r !== void 0) return r;
      let i = e(n);
      return (t.set(n, i), i !== n && t.set(i, i), i);
    };
  }
  var te = (e) => {
      let t = new Set(Reflect.ownKeys(e));
      if (e.constructor === Object) return t;
      e instanceof Error && t.delete(`stack`);
      let n = Object.getPrototypeOf(e),
        r = n;
      for (; r !== null && r !== Object.prototype;) {
        let e = Reflect.ownKeys(r);
        for (let n = 0; n < e.length; n++) t.add(e[n]);
        r = Object.getPrototypeOf(r);
      }
      return (
        t.has(`constructor`) &&
          typeof e.constructor == `function` &&
          n === e.constructor.prototype &&
          t.delete(`constructor`),
        t
      );
    },
    ne = new WeakSet(),
    re = (e) => new Uint8Array(e.buffer, e.byteOffset, e.byteLength),
    ie = 0,
    ae = () => {
      ie++;
    };
  function oe(e) {
    return typeof e == `string`;
  }
  function se(e) {
    return typeof e == `number`;
  }
  function ce(e) {
    return typeof e == `boolean`;
  }
  function le(e) {
    return typeof e == `bigint`;
  }
  function ue(e) {
    return typeof e == `symbol`;
  }
  function de(e) {
    return oe(e) || se(e) || ue(e);
  }
  function fe(e) {
    return typeof e == `function`;
  }
  function pe(e) {
    return e !== void 0;
  }
  function me(e) {
    return e != null;
  }
  function he(e) {
    return !1;
  }
  function ge(e) {
    return !0;
  }
  function _e(e) {
    return typeof e == `object` && !!e && !Array.isArray(e);
  }
  function ve(e) {
    return (typeof e == `object` && !!e) || fe(e);
  }
  var ye = a(2, (e, t) => ve(e) && t in e);
  function be(e) {
    return e instanceof Error;
  }
  var xe = `~effect/Hash`,
    Se = (e) => {
      switch (typeof e) {
        case `number`:
          return Oe(e);
        case `bigint`:
          return ke(e.toString(10));
        case `string`:
          return ke(e);
        case `function`:
        case `object`:
          if (e === null) break;
          if (e instanceof Date)
            return Number.isNaN(e.getTime()) ? ke(`Invalid Date`) : ke(e.toISOString());
          if (e instanceof RegExp) return ke(e.toString());
          {
            if (ne.has(e)) return Ce(e);
            let t = Re.get(e);
            if (t !== void 0) return t;
            if (ze.has(e)) return (ae(), ke(`[Circular]`));
            ze.add(e);
            let n = ie,
              r;
            try {
              r =
                `~effect/Hash` in e
                  ? e[xe]()
                  : typeof e == `function`
                    ? Ce(e)
                    : e instanceof DataView
                      ? Ne(re(e))
                      : Array.isArray(e) || ArrayBuffer.isView(e)
                        ? Ne(e)
                        : e instanceof Map
                          ? Pe(e)
                          : e instanceof Set
                            ? Ie(e)
                            : je(e);
            } finally {
              ze.delete(e);
            }
            return (n === ie && Re.set(e, r), r);
          }
      }
      return Ee(we(ke(String(e))));
    },
    Ce = (e) => (Le.has(e) || Le.set(e, Ee((Math.random() * 4294967296) | 0)), Le.get(e)),
    we = (e) => (
      (e ^= e >>> 16),
      (e = Math.imul(e, 2246822507)),
      (e ^= e >>> 13),
      (e = Math.imul(e, 3266489909)),
      e ^ (e >>> 16)
    ),
    Te = a(2, (e, t) => we(Math.imul(e, 2654435761) + Math.imul(t, 2246822507))),
    Ee = (e) => (e & 3221225471) | ((e >>> 1) & 1073741824),
    De = new DataView(new ArrayBuffer(8)),
    Oe = (e) => {
      let t = e | 0;
      return t === e
        ? Ee(t)
        : (De.setFloat64(0, e === e ? e : NaN), Ee(Te(De.getInt32(0), De.getInt32(4))));
    },
    ke = (e) => {
      let t = 5381,
        n = e.length;
      for (; n;) t = (t * 33) ^ e.charCodeAt(--n);
      return Ee(t);
    },
    Ae = (e, t) => {
      let n = 12289;
      for (let r of t) n ^= Te(Se(r), Se(e[r]));
      return Ee(n);
    },
    je = (e) => Ae(e, te(e)),
    Me = (e, t) => (n) => {
      let r = e;
      for (let e of n) r ^= t(e);
      return Ee(r);
    },
    Ne = (e) => {
      let t = 6151;
      for (let n of e) t = Te(t, Se(n));
      return Ee(t);
    },
    Pe = Me(ke(`Map`), ([e, t]) => Te(Se(e), Se(t))),
    Fe = ke(`Set`),
    Ie = Me(Fe, (e) => Te(Fe, Se(e))),
    Le = new WeakMap(),
    Re = new WeakMap(),
    ze = new WeakSet(),
    Be = `~effect/Equal`;
  function Ve() {
    return arguments.length === 1 ? (e) => He(e, arguments[0]) : He(arguments[0], arguments[1]);
  }
  function He(e, t) {
    if (e === t) return !0;
    if (e == null || t == null) return !1;
    let n = typeof e;
    return n === typeof t
      ? n === `number` && e !== e && t !== t
        ? !0
        : (n !== `object` && n !== `function`) || ne.has(e) || ne.has(t)
          ? !1
          : Ue(e, t)
      : !1;
  }
  function Ue(e, t) {
    let n = Ge.length;
    for (let r = n; r-- > 0;) if (Ge[r] === e && Ke[r] === t) return !0;
    if (n) return We(e, t);
    let r = qe.get(e);
    r || qe.set(e, (r = new WeakMap()));
    let i = r.get(t);
    return (i === void 0 && r.set(t, (i = We(e, t))), i);
  }
  function We(e, t) {
    (Ge.push(e), Ke.push(t));
    try {
      return Je(e, t);
    } finally {
      (Ge.pop(), Ke.pop());
    }
  }
  var Ge = [],
    Ke = [],
    qe = new WeakMap();
  function Je(e, t) {
    if (Se(e) !== Se(t)) return !1;
    if (e instanceof Date) {
      if (!(t instanceof Date)) return !1;
      let n = e.getTime(),
        r = t.getTime();
      return n === r || (Number.isNaN(n) && Number.isNaN(r));
    }
    if (e instanceof RegExp) return t instanceof RegExp && e.toString() === t.toString();
    let n = tt(e);
    if (n !== tt(t) || (typeof e == `function` && !n)) return !1;
    if (n) return e[Be](t);
    if (Array.isArray(e)) return !Array.isArray(t) || e.length !== t.length ? !1 : Ye(e, t);
    if (ArrayBuffer.isView(e)) {
      let n = e instanceof DataView;
      return !ArrayBuffer.isView(t) || e.byteLength !== t.byteLength || n !== t instanceof DataView
        ? !1
        : n
          ? Xe(re(e), re(t))
          : Xe(e, t);
    }
    return e instanceof Map
      ? !(t instanceof Map) || e.size !== t.size
        ? !1
        : Qe(e, t, $e, et)
      : e instanceof Set
        ? !(t instanceof Set) || e.size !== t.size
          ? !1
          : Qe(e, t, Se, He)
        : Ze(e, t);
  }
  function Ye(e, t) {
    for (let n = 0; n < e.length; n++) if (!He(e[n], t[n])) return !1;
    return !0;
  }
  function Xe(e, t) {
    if (e.length !== t.length) return !1;
    for (let n = 0; n < e.length; n++) if (e[n] !== t[n]) return !1;
    return !0;
  }
  function Ze(e, t) {
    let n = te(e),
      r = te(t);
    if (n.size !== r.size) return !1;
    for (let i of n) if (!r.has(i) || !He(e[i], t[i])) return !1;
    return !0;
  }
  function Qe(e, t, n, r) {
    let i = new Map(),
      a = 0;
    for (let e of t) {
      let t = n(e),
        r = i.get(t);
      (r ? r.push(e) : i.set(t, [e]), a++);
    }
    outer: for (let t of e) {
      let e = i.get(n(t));
      if (e) {
        for (let n = 0; n < e.length; n++)
          if (r(t, e[n])) {
            ((e[n] = e[e.length - 1]), e.pop(), a--);
            continue outer;
          }
      }
      return !1;
    }
    return a === 0;
  }
  var $e = (e) => Se(e[0]),
    et = (e, t) => He(e[0], t[0]) && He(e[1], t[1]),
    tt = (e) => ye(e, Be),
    nt = Symbol.for(`~effect/Redactable`),
    rt = (e) => ye(e, nt);
  function it(e) {
    return rt(e) ? at(e) : e;
  }
  function at(e) {
    return e[nt](globalThis[`~effect/Fiber/currentFiber`]?.context ?? ct);
  }
  var ot = `~effect/Fiber/currentFiber`,
    st = new Map(),
    ct = {
      "~effect/Context": {},
      base: st,
      depth: 0,
      mapUnsafe: st,
      pipe() {
        return n(this, arguments);
      },
    };
  function lt(e, t) {
    let n = t?.space ?? 0,
      r = new WeakSet(),
      i = n ? (typeof n == `number` ? ` `.repeat(n) : n) : ``,
      a = (e) => i.repeat(e),
      o = (e, t) => {
        let n = e?.constructor;
        return n && n !== Object.prototype.constructor && n.name ? `${n.name}(${t})` : t;
      },
      s = (e) => {
        try {
          return Reflect.ownKeys(e);
        } catch {
          return [`[ownKeys threw]`];
        }
      };
    function c(e, t = 0) {
      try {
        return l(e, t);
      } catch {
        return (
          ((typeof e == `object` && e) || typeof e == `function`) && r.delete(e),
          `[inspection threw]`
        );
      }
    }
    function l(e, n = 0) {
      if (typeof e == `string`) return JSON.stringify(e);
      if (typeof e == `number` || e == null || typeof e == `boolean` || typeof e == `symbol`)
        return String(e);
      if (typeof e == `bigint`) return String(e) + `n`;
      if (typeof e == `object` || typeof e == `function`) {
        if (r.has(e)) return ut;
        r.add(e);
        let l;
        if (nt in e) l = c(at(e), n);
        else if (Array.isArray(e))
          l =
            !i || e.length <= 1
              ? `[${e.map((e) => c(e, n)).join(`,`)}]`
              : `[\n${a(n + 1)}${e
                  .map((e) => c(e, n + 1))
                  .join(
                    `,
` + a(n + 1),
                  )}\n${a(n)}]`;
        else if (e instanceof Date) l = pt(e);
        else if (
          !t?.ignoreToString &&
          ye(e, `toString`) &&
          typeof e.toString == `function` &&
          e.toString !== Object.prototype.toString &&
          e.toString !== Array.prototype.toString
        ) {
          let t = mt(e);
          l = e instanceof Error && e.cause !== void 0 ? `${t} (cause: ${c(e.cause, n)})` : t;
        } else if (Symbol.iterator in e) l = `${e.constructor.name}(${c(Array.from(e), n)})`;
        else {
          let t = s(e);
          if (!i || t.length <= 1) {
            let r = `{${t.map((t) => `${dt(t)}:${c(ht(e, t), n)}`).join(`,`)}}`;
            l = o(e, r);
          } else {
            let r = `{\n${t.map((t) => `${a(n + 1)}${dt(t)}: ${c(ht(e, t), n + 1)}`).join(`,
`)}\n${a(n)}}`;
            l = o(e, r);
          }
        }
        return (r.delete(e), l);
      }
      return String(e);
    }
    return c(e, 0);
  }
  var ut = `[Circular]`;
  function dt(e) {
    return typeof e == `string` ? JSON.stringify(e) : String(e);
  }
  function ft(e) {
    return e.map((e) => `[${dt(e)}]`).join(``);
  }
  function pt(e) {
    try {
      return e.toISOString();
    } catch {
      return `Invalid Date`;
    }
  }
  function mt(e) {
    try {
      let t = e.toString();
      return typeof t == `string` ? t : String(t);
    } catch {
      return `[toString threw]`;
    }
  }
  function ht(e, t) {
    try {
      return e[t];
    } catch {
      return `[property access threw]`;
    }
  }
  function gt(e, t) {
    let n = [];
    return (
      JSON.stringify(
        e,
        function (e, t) {
          let r = Object.getOwnPropertyDescriptor(this, e)?.value,
            i = ye(r, nt) ? it(r) : it(t);
          if (typeof i == `bigint`) return lt(i);
          if (typeof i != `object` || !i) return i;
          let a =
            i instanceof Error && !ye(i, `toJSON`) ? { ...i, name: i.name, message: i.message } : i;
          for (; n.length > 0 && n[n.length - 1] !== this;) n.pop();
          if (n.includes(i)) return;
          if ((n.push(i), a !== i && n.push(a), !_t(a) || vt(a))) return a;
          let o = new Proxy(a, {
            get(e, t) {
              let n = Reflect.get(e, t, e);
              return Object.getOwnPropertyDescriptor(e, t)?.get === void 0 ? n : it(n);
            },
          });
          return (n.push(o), o);
        },
        t?.space,
      ) ?? `null`
    );
  }
  function _t(e) {
    for (let t of Object.getOwnPropertyNames(e))
      if (Object.getOwnPropertyDescriptor(e, t)?.get !== void 0) return !0;
    return !1;
  }
  function vt(e) {
    for (let t of [Number.prototype.valueOf, Boolean.prototype.valueOf, String.prototype.valueOf])
      try {
        return (Reflect.apply(t, e, []), !0);
      } catch {}
    return !1;
  }
  var yt = Symbol.for(`nodejs.util.inspect.custom`),
    bt = (e) => {
      try {
        return (
          (e = it(e)),
          ye(e, `toJSON`) && fe(e.toJSON) && e.toJSON.length === 0
            ? e.toJSON()
            : Array.isArray(e)
              ? e.map(bt)
              : e
        );
      } catch {
        return `[toJSON threw]`;
      }
    },
    xt = {
      toJSON() {
        return bt(this);
      },
      [yt]() {
        return this.toJSON();
      },
      toString() {
        return lt(this.toJSON());
      },
    },
    St = (() => {
      let e = Object.getOwnPropertyDescriptor(Error, `stackTraceLimit`);
      return e === void 0
        ? Object.isExtensible(Error)
        : Object.hasOwn(e, `writable`)
          ? e.writable === !0
          : e.set !== void 0;
    })(),
    Ct = () => Error.stackTraceLimit,
    wt = (e) => {
      St && (Error.stackTraceLimit = e);
    },
    Tt = class {
      constructor(e) {
        ((this.value = e), (this.done = !1));
      }
      next(e) {
        return this.done
          ? ((this.value = e), this)
          : ((this.done = !0), { value: this.value, done: !1 });
      }
    },
    Et = (() => {
      let e = `~effect/Utils/internal`,
        t = { [e]: (e) => e() },
        n = {
          [e]: (e) => {
            try {
              return e();
            } finally {
            }
          },
        };
      return Ct() !== 0 && t[e](() => Error().stack)?.includes(e) === !0 ? t[e] : n[e];
    })();
  function Dt(e, t, n) {
    t === `__proto__`
      ? Object.defineProperty(e, t, { value: n, writable: !0, enumerable: !0, configurable: !0 })
      : (e[t] = n);
  }
  function Ot(e, t) {
    for (let n of Reflect.ownKeys(t))
      Object.prototype.propertyIsEnumerable.call(t, n) && Dt(e, n, t[n]);
  }
  var kt = `~effect/Effect`,
    At = `~effect/Exit`,
    jt = { _A: o, _E: o, _R: o },
    Mt = `${kt}/identifier`,
    p = `${kt}/args`,
    Nt = `${kt}/evaluate`,
    Pt = `${kt}/successCont`,
    Ft = `${kt}/failureCont`,
    It = `${kt}/ensureCont`,
    Lt = Symbol.for(`effect/Effect/Yield`),
    Rt = {
      pipe() {
        return n(this, arguments);
      },
      toJSON() {
        return { ...this };
      },
      toString() {
        return lt(this.toJSON(), { ignoreToString: !0, space: 2 });
      },
      [yt]() {
        return this.toJSON();
      },
    },
    zt = {
      [kt]: jt,
      ...Rt,
      [Symbol.iterator]() {
        return new Tt(this);
      },
      toJSON() {
        return { _id: `Effect`, op: this[Mt], ...(p in this ? { args: this[p] } : void 0) };
      },
    },
    Bt = (e) => ye(e, kt),
    Vt = (e) => ye(e, At),
    Ht = `~effect/Cause`,
    Ut = `~effect/Cause/Reason`,
    Wt = (e) => ye(e, Ht),
    Gt = class {
      constructor(e) {
        ((this[Ht] = Ht), (this.reasons = e));
      }
      pipe() {
        return n(this, arguments);
      }
      toJSON() {
        return { _id: `Cause`, failures: this.reasons.map((e) => e.toJSON()) };
      }
      toString() {
        return `Cause(${lt(this.reasons)})`;
      }
      [yt]() {
        return this.toJSON();
      }
      [Be](e) {
        return (
          Wt(e) &&
          this.reasons.length === e.reasons.length &&
          this.reasons.every((t, n) => Ve(t, e.reasons[n]))
        );
      }
      [xe]() {
        return Ne(this.reasons);
      }
    },
    Kt = new WeakMap(),
    qt = class {
      [Ut];
      annotations;
      _tag;
      constructor(e, t, n) {
        if (
          ((this[Ut] = Ut), (this._tag = e), t !== Jt && typeof n == `object` && n && t.size > 0)
        ) {
          let e = Kt.get(n);
          (e && (t = new Map([...e, ...t])), Kt.set(n, t));
        }
        this.annotations = t;
      }
      annotate(e, t) {
        if (e.mapUnsafe.size === 0) return this;
        let n = new Map(this.annotations);
        e.mapUnsafe.forEach((e, r) => {
          (t?.overwrite !== !0 && n.has(r)) || n.set(r, e);
        });
        let r = Object.assign(Object.create(Object.getPrototypeOf(this)), this);
        return ((r.annotations = n), r);
      }
      pipe() {
        return n(this, arguments);
      }
      toString() {
        return lt(this);
      }
      [yt]() {
        return this.toString();
      }
    },
    Jt = new Map(),
    Yt = class extends qt {
      constructor(e, t = Jt) {
        (super(`Fail`, t, e), (this.error = e));
      }
      toString() {
        return `Fail(${lt(this.error)})`;
      }
      toJSON() {
        return { _tag: `Fail`, error: this.error };
      }
      [Be](e) {
        return rn(e) && Ve(this.error, e.error) && Ve(this.annotations, e.annotations);
      }
      [xe]() {
        return Te(ke(this._tag))(Te(Se(this.error))(Se(this.annotations)));
      }
    },
    Xt = (e) => new Gt(e),
    Zt = (e, t) => {
      let n = new Map(),
        r = [];
      for (let i of e.concat(t)) {
        let e = Se(i),
          t = n.get(e);
        if (t === void 0) n.set(e, [i]);
        else if (t.some((e) => Ve(e, i))) continue;
        else t.push(i);
        r.push(i);
      }
      return r;
    },
    Qt = a(2, (e, t) => {
      if (e.reasons.length === 0) return t;
      if (t.reasons.length === 0) return e;
      let n = new Gt(Zt(e.reasons, t.reasons));
      return Ve(e, n) ? e : n;
    }),
    $t = (e) => new Gt([new Yt(e)]),
    en = class extends qt {
      constructor(e, t = Jt) {
        (super(`Die`, t, e), (this.defect = e));
      }
      toString() {
        return `Die(${lt(this.defect)})`;
      }
      toJSON() {
        return { _tag: `Die`, defect: this.defect };
      }
      [Be](e) {
        return an(e) && Ve(this.defect, e.defect) && Ve(this.annotations, e.annotations);
      }
      [xe]() {
        return Te(ke(this._tag))(Te(Se(this.defect))(Se(this.annotations)));
      }
    },
    tn = (e) => new Gt([new en(e)]),
    nn = a(
      (e) => Wt(e[0]),
      (e, t, n) => (t.mapUnsafe.size === 0 ? e : new Gt(e.reasons.map((e) => e.annotate(t, n)))),
    ),
    rn = (e) => e._tag === `Fail`,
    an = (e) => e._tag === `Die`,
    on = (e) => e._tag === `Interrupt`;
  function sn(e) {
    return gn(`Effect.evaluate: Not implemented`);
  }
  var cn = (e) => ({ ...zt, [Mt]: e.op, [Nt]: e[Nt] ?? sn, [Pt]: e[Pt], [Ft]: e[Ft], [It]: e[It] }),
    ln = (e) => {
      let t = cn(e),
        n = function (e) {
          this[p] = e;
        };
      return (
        (n.prototype = t),
        function (e) {
          return new n(e);
        }
      );
    },
    un = (e) => {
      let t = {
          [At]: At,
          _tag: e.op,
          get [e.prop]() {
            return this[p];
          },
          ...cn(e),
          toString() {
            return `${e.op}(${lt(this[p])})`;
          },
          toJSON() {
            return { _id: `Exit`, _tag: e.op, [e.prop]: this[p] };
          },
          [Be](e) {
            return Vt(e) && e._tag === this._tag && Ve(this[p], e[p]);
          },
          [xe]() {
            return Te(ke(e.op), Se(this[p]));
          },
        },
        n = function (e) {
          this[p] = e;
        };
      return (
        (n.prototype = t),
        function (e) {
          return new n(e);
        }
      );
    },
    dn = un({
      op: `Success`,
      prop: `value`,
      [Nt](e) {
        let t = e.getCont(Pt);
        return t ? t[Pt](this[p], e, this) : e.yieldWith(this);
      },
    }),
    fn = { key: `effect/Cause/StackTrace` },
    pn = { key: `effect/Cause/InterruptorStackTrace` },
    mn = un({
      op: `Failure`,
      prop: `cause`,
      [Nt](e) {
        let t = this[p],
          n = !1;
        e.cache.stackFrame &&
          ((t = nn(t, { mapUnsafe: new Map([[fn.key, e.cache.stackFrame]]) })), (n = !0));
        let r = e.getCont(Ft),
          i = e._interruptedCause;
        if (i && e.interruptible) {
          let a = !1;
          for (; r && e.interruptible;) ((a ||= Mt in r), (r = e.getCont(Ft)));
          (a && (t = Xt(t.reasons.filter((e) => e._tag !== `Fail`))), (t = Qt(t, i)), (n = !0));
        }
        return r ? r[Ft](t, e, n ? void 0 : this) : e.yieldWith(n ? mn(t) : this);
      },
    }),
    hn = (e) => mn($t(e)),
    gn = (e) => mn(tn(e)),
    _n = ln({
      op: `WithFiber`,
      [Nt](e) {
        return this[p](e);
      },
    }),
    vn = (function () {
      class e extends globalThis.Error {}
      let t = cn({
        op: `YieldableError`,
        [Nt]() {
          return hn(this);
        },
      });
      return (delete t.toString, Object.assign(e.prototype, t), e);
    })(),
    yn = (function () {
      let e = Symbol.for(`effect/Data/Error/plainArgs`);
      return class extends vn {
        constructor(t) {
          (super(t?.message, t?.cause ? { cause: t.cause } : void 0),
            t && (Ot(this, t), Object.defineProperty(this, e, { value: t, enumerable: !1 })));
        }
        toJSON() {
          return { ...this[e], ...this };
        }
      };
    })(),
    bn = (e) => {
      class t extends yn {
        _tag = e;
      }
      return ((t.prototype.name = e), t);
    },
    xn = `~effect/Cause/Done`,
    Sn = (e) => cn({ op: e.label, [Nt]: e.evaluate }),
    Cn = (e, t) => e === t,
    wn = () => Cn,
    Tn = `~effect/Option`,
    En = {
      [Tn]: { _A: (e) => e },
      ...Rt,
      [Symbol.iterator]() {
        return new Tt(this);
      },
    },
    Dn = Object.defineProperty(
      Object.assign(Object.create(En), {
        _tag: `Some`,
        _op: `Some`,
        [Be](e) {
          return An(e) && Mn(e) && Ve(this.value, e.value);
        },
        [xe]() {
          return Te(Se(this._tag))(Se(this.value));
        },
        toString() {
          return `some(${lt(this.value)})`;
        },
        toJSON() {
          return { _id: `Option`, _tag: this._tag, value: bt(this.value) };
        },
      }),
      "valueOrUndefined",
      {
        get() {
          return this.value;
        },
      },
    ),
    On = Se(`None`),
    kn = Object.assign(Object.create(En), {
      _tag: `None`,
      _op: `None`,
      valueOrUndefined: void 0,
      [Be](e) {
        return An(e) && jn(e);
      },
      [xe]() {
        return On;
      },
      toString() {
        return `none()`;
      },
      toJSON() {
        return { _id: `Option`, _tag: this._tag };
      },
    }),
    An = (e) => ye(e, Tn),
    jn = (e) => e._tag === `None`,
    Mn = (e) => e._tag === `Some`,
    Nn = Object.create(kn),
    Pn = function (e) {
      this.value = e;
    };
  Pn.prototype = Dn;
  var Fn = (e) => new Pn(e),
    In = `~effect/Result`,
    Ln = {
      [In]: { _A: (e) => e, _E: (e) => e },
      ...Rt,
      [Symbol.iterator]() {
        return new Tt(this);
      },
    },
    Rn = Object.assign(Object.create(Ln), {
      _tag: `Success`,
      _op: `Success`,
      [Be](e) {
        return Bn(e) && Hn(e) && Ve(this.success, e.success);
      },
      [xe]() {
        return Te(Se(this._tag))(Se(this.success));
      },
      toString() {
        return `success(${lt(this.success)})`;
      },
      toJSON() {
        return { _id: `Result`, _tag: this._tag, value: bt(this.success) };
      },
    }),
    zn = Object.assign(Object.create(Ln), {
      _tag: `Failure`,
      _op: `Failure`,
      [Be](e) {
        return Bn(e) && Vn(e) && Ve(this.failure, e.failure);
      },
      [xe]() {
        return Te(Se(this._tag))(Se(this.failure));
      },
      toString() {
        return `failure(${lt(this.failure)})`;
      },
      toJSON() {
        return { _id: `Result`, _tag: this._tag, failure: bt(this.failure) };
      },
    }),
    Bn = (e) => ye(e, In),
    Vn = (e) => e._tag === `Failure`,
    Hn = (e) => e._tag === `Success`,
    Un = function (e) {
      this.failure = e;
    };
  Un.prototype = zn;
  var Wn = (e) => new Un(e),
    Gn = function (e) {
      this.success = e;
    };
  Gn.prototype = Rn;
  var Kn = (e) => new Gn(e);
  function qn(e) {
    return (t, n) => (t === n ? 0 : e(t, n));
  }
  var Jn = qn((e, t) => (e < t ? -1 : 1)),
    Yn = qn((e, t) =>
      globalThis.Number.isNaN(e) && globalThis.Number.isNaN(t)
        ? 0
        : globalThis.Number.isNaN(e)
          ? -1
          : globalThis.Number.isNaN(t)
            ? 1
            : e < t
              ? -1
              : 1,
    ),
    Xn = (e) => a(2, (t, n) => e(t, n) === -1),
    Zn = (e) => a(2, (t, n) => e(t, n) === 1),
    Qn = (e) => a(2, (t, n) => e(t, n) !== 1),
    $n = (e) => a(2, (t, n) => e(t, n) !== -1),
    er = () => Nn,
    tr = Fn,
    nr = An,
    rr = jn,
    ir = Mn,
    ar = a(2, (e, { onNone: t, onSome: n }) => (rr(e) ? t() : n(e.value))),
    or = a(2, (e, t) => (rr(e) ? t() : e.value)),
    sr = (e) => (e === null ? er() : tr(e)),
    cr = or(l),
    lr =
      (e) =>
      (...t) => {
        try {
          return tr(e(...t));
        } catch {
          return er();
        }
      },
    ur = a(2, (e, t) => (rr(e) ? er() : t(e.value) ? tr(e.value) : er())),
    dr = `~effect/Context/Service`,
    fr = function () {
      function e() {}
      let t = e;
      Object.setPrototypeOf(t, pr);
      let n = (e, n) => (
        (t.key = e),
        n?.defaultValue && ((t[hr] = hr), (t.defaultValue = n.defaultValue)),
        n?.make && (t.make = n.make),
        n?.fiberCached && mr.add(e),
        t
      );
      return arguments.length > 0 ? n(arguments[0], arguments[1]) : n;
    },
    pr = {
      [dr]: dr,
      ...Sn({
        label: `Service`,
        evaluate(e) {
          return dn(Rr(e.context, this));
        },
      }),
      toJSON() {
        return { _id: `Service`, key: this.key };
      },
      of(e) {
        return e;
      },
      context(e) {
        return Mr(this, e);
      },
      use(e) {
        return _n((t) => e(Rr(t.context, this)));
      },
      useSync(e) {
        return _n((t) => dn(e(Rr(t.context, this))));
      },
    },
    mr = new Set(),
    hr = `~effect/Context/Reference`,
    gr = `~effect/Context`,
    _r = 8,
    vr = 8,
    yr = (e, t, n, r) => {
      let i = Object.create(Er);
      return (
        (i.cacheRoot = e ?? i),
        (i.base = t),
        (i.overlay = n),
        (i.depth = r),
        (i._flat = void 0),
        (i.baseHits = 0),
        i
      );
    },
    br = (e, t) => {
      t && (br(e, t.parent), e.set(t.key, t.value));
    },
    xr = (e) => {
      if (e._flat) return e._flat;
      if (!e.overlay) return (e._flat = e.base);
      let t = new Map(e.base);
      return (br(t, e.overlay), (e._flat = t));
    },
    Sr = (e, t) => {
      let n = new Map(e.mapUnsafe);
      return (t(n), Tr(n));
    },
    Cr = Symbol(),
    wr = (e, t) => {
      let n = e;
      for (let e = n.overlay; e; e = e.parent) if (e.key === t) return e.value;
      let r = n.base.get(t);
      return r === void 0 && !n.base.has(t)
        ? Cr
        : (n.overlay &&
            ++n.baseHits >= n.base.size &&
            n.baseHits >= vr &&
            ((n.base = xr(n)), (n.overlay = void 0), (n.depth = 0)),
          r);
    },
    Tr = (e) => yr(void 0, e, void 0, 0),
    Er = {
      get mapUnsafe() {
        return xr(this);
      },
      ...Rt,
      [gr]: { _Services: (e) => e },
      toJSON() {
        return {
          _id: `Context`,
          services: Array.from(this.mapUnsafe).map(([e, t]) => ({ key: e, value: t })),
        };
      },
      [Be](e) {
        if (!Or(e)) return !1;
        let t = this.mapUnsafe,
          n = e.mapUnsafe;
        if (t.size !== n.size) return !1;
        for (let [e, r] of t) if (!n.has(e) || !Ve(r, n.get(e))) return !1;
        return !0;
      },
      [xe]() {
        return Oe(this.mapUnsafe.size);
      },
    },
    Dr = (e, t) => e.cacheRoot === t.cacheRoot,
    Or = (e) => ye(e, gr),
    kr = (e) => !!e[hr],
    Ar = () => jr,
    jr = Tr(new Map()),
    Mr = (e, t) => Tr(new Map([[e.key, t]])),
    Nr = a(3, (e, t, n) => Pr(e, t.key, n)),
    Pr = (e, t, n) => {
      let r = e,
        i = mr.has(t) ? void 0 : r.cacheRoot;
      if (r.depth >= _r) {
        let e = new Map(r.mapUnsafe);
        return (e.set(t, n), yr(i, e, void 0, 0));
      }
      return yr(i, r.base, { key: t, value: n, parent: r.overlay }, r.depth + 1);
    },
    Fr = a(2, (e, t) => Ir(e, t.key)),
    Ir = (e, t) => {
      let n = wr(e, t);
      return n === Cr ? void 0 : n;
    },
    Lr = a(2, (e, t) => {
      let n = wr(e, t.key);
      if (n === Cr) {
        if (kr(t)) return Br(t);
        throw Vr(t);
      }
      return n;
    }),
    Rr = Lr,
    zr = `~effect/Context/defaultValue`,
    Br = (e) => (zr in e ? e[zr] : (e[zr] = e.defaultValue())),
    Vr = (e) => {
      let t = Error(`Service not found${e.key ? `: ${String(e.key)}` : ``}`);
      if (t.stack) {
        let e = t.stack.split(`
`);
        (e.splice(1, 3),
          (t.stack = e.join(`
`)));
      }
      return t;
    },
    Hr = a(2, (e, t) =>
      e.mapUnsafe.size === 0
        ? t
        : t.mapUnsafe.size === 0
          ? e
          : Sr(e, (e) => t.mapUnsafe.forEach((t, n) => e.set(n, t))),
    ),
    Ur = (...e) => {
      let t = new Map();
      for (let n = 0; n < e.length; n++)
        e[n].mapUnsafe.forEach((e, n) => {
          t.set(n, e);
        });
      return Tr(t);
    },
    Wr = fr,
    Gr = `~effect/Duration`,
    Kr = BigInt(0),
    qr = BigInt(1),
    Jr = BigInt(2),
    Yr = BigInt(10),
    Xr = BigInt(1e3),
    Zr = (e) => BigInt(e < 0 ? Math.ceil(e - 0.5) : Math.floor(e + 0.5)),
    Qr = (e) => Zr(e * 1e6),
    $r = (e, t) => {
      let n = e.indexOf(`.`);
      if (n === -1) return BigInt(e) * t;
      let r = e[0] === `-`,
        i = e.slice(n + 1),
        a = Yr ** BigInt(i.length),
        o = (BigInt(e.slice(+!!r, n)) * a + BigInt(i)) * t,
        s = o / a + ((o % a) * Jr >= a ? qr : Kr);
      return r ? -s : s;
    },
    ei = /^(-?\d+(?:\.\d+)?)\s+(nanos?|micros?|millis?|seconds?|minutes?|hours?|days?|weeks?)$/,
    ti = (e) => {
      switch (typeof e) {
        case `number`:
          return pi(e);
        case `bigint`:
          return fi(e);
        case `string`: {
          if (e === `Infinity`) return ui;
          if (e === `-Infinity`) return di;
          let t = ei.exec(e);
          if (!t) break;
          let [n, r, i] = t;
          if (i === `nano` || i === `nanos`) return fi($r(r, qr));
          if (i === `micro` || i === `micros`) return fi($r(r, Xr));
          let a = Number(r);
          switch (i) {
            case `milli`:
            case `millis`:
              return pi(a);
            case `second`:
            case `seconds`:
              return mi(a);
            case `minute`:
            case `minutes`:
              return hi(a);
            case `hour`:
            case `hours`:
              return gi(a);
            case `day`:
            case `days`:
              return _i(a);
            case `week`:
            case `weeks`:
              return vi(a);
          }
          break;
        }
        case `object`: {
          if (e === null) break;
          if (Gr in e) return e;
          if (Array.isArray(e))
            return e.length !== 2 || !e.every(se)
              ? ni(e)
              : Number.isNaN(e[0]) || Number.isNaN(e[1])
                ? li
                : e[0] === -1 / 0 || e[1] === -1 / 0
                  ? di
                  : e[0] === 1 / 0 || e[1] === 1 / 0
                    ? ui
                    : si(Zr(e[0] * 1e9 + e[1]));
          let t = e,
            n = 0;
          return (
            t.weeks && (n += t.weeks * 6048e5),
            t.days && (n += t.days * 864e5),
            t.hours && (n += t.hours * 36e5),
            t.minutes && (n += t.minutes * 6e4),
            t.seconds && (n += t.seconds * 1e3),
            t.milliseconds && (n += t.milliseconds),
            !t.microseconds && !t.nanoseconds
              ? si(n)
              : si(Zr(n * 1e6 + (t.microseconds ?? 0) * 1e3 + (t.nanoseconds ?? 0)))
          );
        }
      }
      return ni(e);
    },
    ni = (e) => {
      throw Error(`Invalid Input: ${e}`);
    },
    ri = { _tag: `Millis`, millis: 0 },
    ii = { _tag: `Infinity` },
    ai = { _tag: `NegativeInfinity` },
    oi = {
      [Gr]: Gr,
      [xe]() {
        switch (this.value._tag) {
          case `Millis`: {
            let e = this.value.millis * 1e6;
            return Number.isFinite(e) ? Se(Zr(e)) : Oe(this.value.millis);
          }
          case `Nanos`:
            return Se(this.value.nanos);
          default:
            return je(this.value);
        }
      },
      [Be](e) {
        return ci(e) && wi(this, e);
      },
      toString() {
        switch (this.value._tag) {
          case `Infinity`:
            return `Infinity`;
          case `NegativeInfinity`:
            return `-Infinity`;
          case `Nanos`:
            return `${this.value.nanos} nanos`;
          case `Millis`:
            return `${this.value.millis} millis`;
        }
      },
      toJSON() {
        switch (this.value._tag) {
          case `Millis`:
            return { _id: `Duration`, _tag: `Millis`, millis: this.value.millis };
          case `Nanos`:
            return { _id: `Duration`, _tag: `Nanos`, nanos: String(this.value.nanos) };
          case `Infinity`:
            return { _id: `Duration`, _tag: `Infinity` };
          case `NegativeInfinity`:
            return { _id: `Duration`, _tag: `NegativeInfinity` };
        }
      },
      [yt]() {
        return this.toJSON();
      },
      pipe() {
        return n(this, arguments);
      },
    },
    si = (e) => {
      let t = Object.create(oi);
      return (
        (t.value =
          typeof e == `number`
            ? isNaN(e) || e === 0 || Object.is(e, -0)
              ? ri
              : Number.isFinite(e)
                ? Number.isInteger(e)
                  ? { _tag: `Millis`, millis: e }
                  : { _tag: `Nanos`, nanos: Qr(e) }
                : e > 0
                  ? ii
                  : ai
            : e === Kr
              ? ri
              : { _tag: `Nanos`, nanos: e }),
        t
      );
    },
    ci = (e) => ye(e, Gr),
    li = si(0),
    ui = si(1 / 0),
    di = si(-1 / 0),
    fi = (e) => si(e),
    pi = (e) => si(e),
    mi = (e) => si(e * 1e3),
    hi = (e) => si(e * 6e4),
    gi = (e) => si(e * 36e5),
    _i = (e) => si(e * 864e5),
    vi = (e) => si(e * 6048e5),
    yi = (e) =>
      xi(ti(e), {
        onMillis: o,
        onNanos: (e) => Number(e) / 1e6,
        onInfinity: () => 1 / 0,
        onNegativeInfinity: () => -1 / 0,
      }),
    bi = (e) => {
      let t = ti(e);
      switch (t.value._tag) {
        case `Infinity`:
        case `NegativeInfinity`:
          throw Error(`Cannot convert infinite duration to nanos`);
        case `Nanos`:
          return t.value.nanos;
        case `Millis`:
          return Qr(t.value.millis);
      }
    },
    xi = a(2, (e, t) => {
      switch (e.value._tag) {
        case `Millis`:
          return t.onMillis(e.value.millis);
        case `Nanos`:
          return t.onNanos(e.value.nanos);
        case `Infinity`:
          return t.onInfinity();
        case `NegativeInfinity`:
          return (t.onNegativeInfinity ?? t.onInfinity)();
      }
    }),
    Si = a(3, (e, t, n) =>
      e.value._tag === `Infinity` ||
      e.value._tag === `NegativeInfinity` ||
      t.value._tag === `Infinity` ||
      t.value._tag === `NegativeInfinity`
        ? n.onInfinity(e, t)
        : e.value._tag === `Millis`
          ? t.value._tag === `Millis`
            ? n.onMillis(e.value.millis, t.value.millis)
            : n.onNanos(bi(e), t.value.nanos)
          : n.onNanos(e.value.nanos, bi(t)),
    ),
    Ci = (e, t) =>
      Si(e, t, {
        onMillis: (e, t) => e === t,
        onNanos: (e, t) => e === t,
        onInfinity: (e, t) => e.value._tag === t.value._tag,
      }),
    wi = a(2, (e, t) => Ci(e, t)),
    Ti = (e) => e.length > 0,
    Ei = Kn,
    Di = Wn,
    Oi = Vn,
    ki = a(2, (e, { onFailure: t, onSuccess: n }) => (Oi(e) ? t(e.failure) : n(e.success))),
    Ai = a(2, (e, t) => Object.hasOwn(e, t)),
    ji = a(2, (e, t) => {
      let n = { ...e };
      for (let r of Mi(e)) Dt(n, r, t(e[r], r));
      return n;
    }),
    Mi = (e) => Object.keys(e),
    Ni = (e) =>
      a(2, (t, n) => {
        for (let r of Mi(t)) if (!Ai(n, r) || !e(t[r], n[r])) return !1;
        return !0;
      }),
    Pi = (e) => {
      let t = Ni(e);
      return (e, n) => t(e, n) && t(n, e);
    },
    Fi = globalThis.Array,
    Ii = (e) => (Fi.isArray(e) ? e : Fi.from(e)),
    Li = (e) => (Fi.isArray(e) ? e : [e]),
    Ri = a(2, (e, t) => [...e, t]);
  Fi.isArray;
  var zi = Ti,
    Bi = Ti,
    Vi = () => [],
    Hi = a(2, (e, t) => e.map(t)),
    Ui = Wr(`effect/Scheduler`, { fiberCached: !0, defaultValue: () => new Ji() }),
    Wi = (e) => {
      let t = !1;
      return (
        Promise.resolve().then(() => {
          t || e();
        }),
        () => {
          t = !0;
        }
      );
    },
    Gi =
      `setImmediate` in globalThis
        ? (e) => {
            let t = globalThis.setImmediate(e);
            return () => globalThis.clearImmediate(t);
          }
        : (e) => {
            let t = setTimeout(e, 0);
            return () => clearTimeout(t);
          },
    Ki = (e) => {
      try {
        return Gi(e);
      } catch {
        return Wi(e);
      }
    },
    qi = class {
      buckets = [];
      scheduleTask(e, t) {
        let n = this.buckets,
          r = n.length,
          i,
          a = 0;
        for (; a < r && !(n[a][0] > t); a++) i = n[a];
        i && i[0] === t ? i[1].push(e) : a === r ? n.push([t, [e]]) : n.splice(a, 0, [t, [e]]);
      }
      drain() {
        let e = this.buckets;
        return ((this.buckets = []), e);
      }
    },
    Ji = class {
      executionMode;
      setImmediate;
      constructor(e = `async`, t) {
        ((this.executionMode = e), (this.setImmediate = t ?? (e === `sync` ? Wi : Ki)));
      }
      shouldYield(e) {
        return e.currentOpCount >= e.cache.maxOpsBeforeYield;
      }
      makeDispatcher() {
        return new Yi(this.setImmediate);
      }
    },
    Yi = class {
      tasks = new qi();
      running = void 0;
      setImmediate;
      constructor(e = Ki) {
        this.setImmediate = e;
      }
      scheduleTask(e, t) {
        (this.tasks.scheduleTask(e, t),
          this.running === void 0 && (this.running = this.setImmediate(this.afterScheduled)));
      }
      afterScheduled = () => {
        ((this.running = void 0), this.runTasks());
      };
      runTasks() {
        let e = this.tasks.drain();
        for (let t = 0; t < e.length; t++) {
          let n = e[t][1];
          for (let e = 0; e < n.length; e++) n[e]();
        }
      }
      flush() {
        for (; this.tasks.buckets.length > 0;)
          (this.running !== void 0 && (this.running(), (this.running = void 0)), this.runTasks());
      }
    },
    Xi = Wr(`effect/Scheduler/MaxOpsBeforeYield`, { fiberCached: !0, defaultValue: () => 2048 }),
    Zi = Wr(`effect/Scheduler/PreventSchedulerYield`, { fiberCached: !0, defaultValue: () => !1 }),
    Qi = class extends i {
      constructor(e) {
        (super(), e && Ot(this, e));
      }
    },
    $i = bn,
    ea = `~effect/encoding/EncodingError`,
    ta = class extends $i(`EncodingError`) {
      [ea] = ea;
    },
    na = `effect/Tracer/ParentSpan`,
    ra = `effect/Tracer`,
    ia = `effect/Metric/FiberRuntimeMetrics`,
    aa = Wr(`effect/References/CurrentStackFrame`, { fiberCached: !0, defaultValue: u }),
    oa = Wr(`effect/References/TracerEnabled`, { fiberCached: !0, defaultValue: c }),
    sa = Wr(`effect/References/CurrentLogLevel`, { fiberCached: !0, defaultValue: () => `Info` }),
    ca = Wr(`effect/References/MinimumLogLevel`, { fiberCached: !0, defaultValue: () => `Info` }),
    la = class extends qt {
      constructor(e, t = Jt) {
        (super(`Interrupt`, t, `Interrupted`), (this.fiberId = e));
      }
      toString() {
        return `Interrupt(${this.fiberId})`;
      }
      toJSON() {
        return { _tag: `Interrupt`, fiberId: this.fiberId };
      }
      [Be](e) {
        return on(e) && this.fiberId === e.fiberId && this.annotations === e.annotations;
      }
      [xe]() {
        return Te(ke(`${this._tag}:${this.fiberId}`))(Ce(this.annotations));
      }
    },
    ua = (e) => new Gt([new la(e)]),
    da = (e) => {
      for (let t = 0; t < e.reasons.length; t++) {
        let n = e.reasons[t];
        if (n._tag === `Fail`) return Ei(n.error);
      }
      return Di(e);
    },
    fa = (e) => e.reasons.some(on),
    pa = a(2, (e, t) => {
      let n = !1,
        r = e.reasons.map((e) => (rn(e) ? ((n = !0), new Yt(t(e.error), e.annotations)) : e));
      return n ? Xt(r) : e;
    }),
    ma = `~effect/Fiber`,
    ha = { _A: o, _E: o },
    ga = { id: 0 },
    _a = (() => {
      try {
        return globalThis.process?.getBuiltinModule?.(`node:async_hooks`)?.AsyncResource;
      } catch {
        return;
      }
    })(),
    va = () => (_a === void 0 ? void 0 : new _a(`effect/Fiber`)),
    ya = () => globalThis[ot],
    ba = class {
      constructor(e, t = !0) {
        (this.setContext(e),
          (this.id = ++ga.id),
          (this.currentOpCount = 0),
          (this.interruptible = t),
          (this._stack = []),
          (this._observers = void 0),
          (this._exit = void 0),
          (this._children = void 0),
          (this._interruptedCause = void 0),
          (this._yielded = void 0),
          (this._running = !1),
          (this._deferredInterrupt = !1),
          (this._parent = void 0),
          (this._asyncContext = void 0),
          this.cache.runtimeMetrics?.recordFiberStart(this.context));
      }
      get [ma]() {
        return ha;
      }
      get currentDispatcher() {
        return (this._dispatcher ??= this.cache.scheduler.makeDispatcher());
      }
      getRef(e) {
        return Rr(this.context, e);
      }
      addObserver(e) {
        return this._exit
          ? (e(this._exit), d)
          : (this._observers === void 0 ? (this._observers = [e]) : this._observers.push(e),
            () => this.removeObserver(e));
      }
      removeObserver(e) {
        if (this._exit || this._observers === void 0) return;
        let t = this._observers.indexOf(e);
        t >= 0 && this._observers.splice(t, 1);
      }
      interruptUnsafe(e, t) {
        if (this._exit) return;
        let n = ua(e);
        (this.cache.stackFrame && (n = nn(n, Mr(fn, this.cache.stackFrame))),
          t && (n = nn(n, t)),
          (this._interruptedCause = this._interruptedCause ? Qt(this._interruptedCause, n) : n),
          this.interruptible &&
            (this._running
              ? (this._deferredInterrupt = !0)
              : this.evaluate(ka(this._interruptedCause))));
      }
      pollUnsafe() {
        return this._exit;
      }
      evaluate(e) {
        if (this._exit) return;
        if (this._asyncContext !== void 0) {
          let t = this._asyncContext;
          return ((this._asyncContext = void 0), t.runInAsyncScope(this.evaluate, this, e));
        }
        if (this._yielded !== void 0) {
          let e = this._yielded;
          ((this._yielded = void 0), e());
        }
        let t = this.runLoop(e);
        if (t === Lt) {
          this._asyncContext = va();
          return;
        }
        let n = wa.interruptChildren && wa.interruptChildren(this);
        if (n !== void 0) return this.evaluate(oo(n, () => t));
        if (
          ((this._exit = t),
          this.cache.runtimeMetrics?.recordFiberEnd(this.context, this._exit),
          (this._parent &&= (this._parent._children?.delete(this), void 0)),
          this._observers !== void 0)
        ) {
          let e = this._observers;
          this._observers = void 0;
          for (let n = 0; n < e.length; n++) e[n](t);
        }
        ((this._stack.length = 0), (this._children = void 0), (this.context = Ar()));
      }
      runLoop(e) {
        let t = globalThis[ot];
        globalThis[ot] = this;
        let n = this._running;
        this._running = !0;
        let r = !1,
          i = e;
        this.currentOpCount = 0;
        try {
          for (;;)
            try {
              (this._deferredInterrupt &&
                ((this._deferredInterrupt = !1), (i = ka(this._interruptedCause))),
                this.currentOpCount++);
              let e = this.cache;
              if (!r && !e.preventYield && e.scheduler.shouldYield(this)) {
                r = !0;
                let e = i;
                i = oo(Pa, () => e);
              }
              if (((i = e.tracerContext ? e.tracerContext(i, this) : i[Nt](this)), i === Lt)) {
                let e = this._yielded;
                if (At in e) return ((this._deferredInterrupt = !1), (this._yielded = void 0), e);
                if (this._deferredInterrupt) {
                  ((this._yielded = void 0), e());
                  continue;
                }
                return Lt;
              }
            } catch (e) {
              if (!ye(i, Nt)) return gn(`Fiber.runLoop: Not a valid effect: ${String(i)}`);
              i = gn(e);
            }
        } finally {
          ((this._running = n), (globalThis[ot] = t));
        }
      }
      getCont(e) {
        if (this._deferredInterrupt) return ((this._deferredInterrupt = !1), Sa);
        for (;;) {
          let t = this._stack.pop();
          if (!t) return;
          let n = t[It];
          if (n !== void 0) {
            let r = n.call(t, this);
            if (r) return ((r[e] = r), r);
          }
          if (t[e]) return t;
        }
      }
      succeedWith(e) {
        if (!(++this.currentOpCount & (Ca - 1))) return dn(e);
        let t = this.getCont(Pt);
        return t ? t[Pt](e, this) : this.yieldWith(dn(e));
      }
      yieldWith(e) {
        return ((this._yielded = e), Lt);
      }
      children() {
        return (this._children ??= new Set());
      }
      pipe() {
        return n(this, arguments);
      }
      setContext(e) {
        let t = this.context;
        if (((this.context = e), t !== void 0 && Dr(t, e))) return;
        let n = e.cacheRoot,
          r = (n._fiberCache ??= xa(e));
        (this.cache?.scheduler !== r.scheduler && (this._dispatcher = void 0), (this.cache = r));
      }
      get currentSpanLocal() {
        let e = this.cache.span;
        return e?._tag === `Span` ? e : void 0;
      }
    },
    xa = (e) => {
      let t = Ir(e, ra);
      return {
        scheduler: Rr(e, Ui),
        tracer: t,
        tracerContext: t ? t.context : void 0,
        tracerEnabled: Rr(e, oa),
        span: Ir(e, na),
        logLevel: Rr(e, sa),
        minimumLogLevel: Rr(e, ca),
        stackFrame: Rr(e, aa),
        runtimeMetrics: Ir(e, ia),
        maxOpsBeforeYield: Rr(e, Xi),
        preventYield: Rr(e, Zi),
      };
    },
    Sa = {
      [Pt](e, t) {
        return ka(t._interruptedCause);
      },
      [Ft](e, t) {
        return ka(t._interruptedCause);
      },
    },
    Ca = 32,
    wa = { interruptChildren: void 0 },
    Ta = (e) => {
      if (!e.cache.stackFrame) return;
      let t = new Map();
      return (t.set(pn.key, e.cache.stackFrame), Tr(t));
    },
    Ea = (e) =>
      Wa((t) => {
        let n = e[Symbol.iterator](),
          r = [],
          i;
        function a() {
          let e = n.next();
          for (; !e.done;) {
            if (e.value._exit) {
              (r.push(e.value._exit), (e = n.next()));
              continue;
            }
            i = e.value.addObserver((e) => {
              (r.push(e), a());
            });
            return;
          }
          t(Oa(r));
        }
        return (a(), ja(() => i?.()));
      }),
    Da = (e) =>
      _n((t) => {
        let n = Ta(t),
          r = Vi();
        for (let i of e) (i.interruptUnsafe(t.id, n), r.push(i));
        return ao(Ea(r));
      }),
    Oa = dn,
    ka = mn,
    Aa = hn,
    ja = ln({
      op: `Sync`,
      [Nt](e) {
        let t = this[p](),
          n = e.getCont(Pt);
        return n ? n[Pt](t, e) : e.yieldWith(dn(t));
      },
    }),
    Ma = ln({
      op: `Suspend`,
      [Nt](e) {
        return this[p]();
      },
    }),
    Na = ki({ onFailure: Aa, onSuccess: Oa }),
    Pa = ln({
      op: `Yield`,
      [Nt](e) {
        let t = !1;
        return (
          e.currentDispatcher.scheduleTask(() => {
            t || e.evaluate(go);
          }, this[p] ?? 0),
          e.yieldWith(() => {
            t = !0;
          })
        );
      },
    })(0),
    Fa = (e) => Oa(tr(e)),
    Ia = Oa(er()),
    La = (e) => Ma(() => ka(e())),
    Ra = (e) => gn(e),
    za = (e) => Ma(() => Aa(e())),
    Ba = Oa(void 0),
    Va = (e) => {
      let t = typeof e == `function` ? e : e.try,
        n = typeof e == `function` ? (e) => new Es(e, `An error occurred in Effect.try`) : e.catch;
      return Ma(() => {
        try {
          return Oa(t());
        } catch (e) {
          return Aa(n(e));
        }
      });
    },
    Ha = (function () {
      let e = cn({
          op: `Async`,
          [Nt](e) {
            let t = !1,
              n = !1,
              r = this.withSignal ? new AbortController() : void 0,
              i = this.register.call(
                e.cache.scheduler,
                (r) => {
                  t || ((t = !0), n ? e.evaluate(r) : (n = r));
                },
                r?.signal,
              );
            return n === !1
              ? ((n = !0),
                (e._yielded = () => {
                  t = !0;
                }),
                (r === void 0 && i === void 0) ||
                  e._stack.push(Ua(() => ((t = !0), r?.abort(), i ?? go))),
                Lt)
              : n;
          },
        }),
        t = function (e, t) {
          ((this.register = e), (this.withSignal = t));
        };
      return (
        (t.prototype = e),
        function (e, n) {
          return new t(e, n);
        }
      );
    })(),
    Ua = ln({
      op: `AsyncFinalizer`,
      [It](e) {
        e.interruptible && ((e.interruptible = !1), e._stack.push(cs));
      },
      [Ft](e, t) {
        return fa(e) ? oo(qo(mn(e), this[p]()), () => ka(e)) : ka(e);
      },
    }),
    Wa = (e) => Ha(e, e.length >= 2),
    Ga = (...e) => {
      if (e.length === 1) {
        let t = e[0];
        return Ma(() => Xa(t()));
      }
      let [t, n] = e;
      return Ma(() => Xa(n.call(t.self)));
    },
    Ka = (e, ...t) => {
      let n =
        t.length === 0
          ? function () {
              return Ma(() => Xa(e.apply(this, arguments)));
            }
          : function () {
              let n = Ma(() => Xa(e.apply(this, arguments)));
              for (let e = 0; e < t.length; e++) n = t[e](n, ...arguments);
              return n;
            };
      return qa(e.length, n);
    },
    qa = (e, t) => Object.defineProperty(t, "length", { value: e, configurable: !0 }),
    Ja = (e, ...t) =>
      qa(
        e.length,
        t.length === 0
          ? function () {
              return Ya(() => e.apply(this, arguments));
            }
          : function () {
              let n = Ya(() => e.apply(this, arguments));
              for (let e of t) n = e(n, ...arguments);
              return n;
            },
      ),
    Ya = (e) => {
      try {
        let t = e(),
          n;
        for (;;) {
          let r = t.next(n);
          if (r.done) return Oa(r.value);
          let i = r.value;
          if (i && i._tag === `Success`) {
            n = i.value;
            continue;
          }
          if (i && i._tag === `Failure`) return r.value;
          {
            let n = !0;
            return Ma(() => (n ? ((n = !1), oo(r.value, (e) => Xa(t, e))) : Ma(() => Xa(e()))));
          }
        }
      } catch (e) {
        return Ra(e);
      }
    },
    Xa = (function () {
      let e = cn({
          op: `Iterator`,
          [Pt](e, t) {
            let n = this.iterator;
            for (;;) {
              let r = n.next(e);
              if (r.done) return Oa(r.value);
              if (!so(r.value)) return (t._stack.push(this), r.value);
              if (r.value._tag === `Failure`) return r.value;
              e = r.value.value;
            }
          },
          [Nt](e) {
            return this[Pt](this.initial, e);
          },
        }),
        t = function (e, t) {
          ((this.iterator = e), (this.initial = t));
        };
      return (
        (t.prototype = e),
        function (e, n) {
          return new t(e, n);
        }
      );
    })(),
    Za = a(2, (e, t) => new eo(e, to, Oa(t))),
    Qa = function (e) {
      return (e._stack.push(this), this[p]);
    },
    $a = cn({ op: `OnSuccess`, [Nt]: Qa }),
    eo = function (e, t, n) {
      ((this[p] = e), (this[Pt] = t), (this.payload = n));
    };
  eo.prototype = $a;
  var to = function () {
      return this.payload;
    },
    no = (() => {
      let e = `~effect/Effect/stackProbe`;
      return (
        {
          [e]: function () {
            return Error().stack;
          },
        }
          [e]()
          ?.includes(`[as ` + e + `]`) === !0
      );
    })(),
    ro = function (e, t) {
      let n = this.payload;
      return t.succeedWith(no ? n(e) : Et(() => n(e)));
    },
    io = function (e) {
      let t = this.payload;
      return t(e);
    },
    ao = (e) => new eo(e, to, go),
    oo = a(2, (e, t) => new eo(e, io, t)),
    so = (e) => e[At] !== void 0,
    co = a(2, (e, t) => (so(e) ? (e._tag === `Success` ? t(e.value) : e) : oo(e, t))),
    lo = a(2, (e, t) => new eo(e, ro, t)),
    uo = a(2, (e, t) => (so(e) ? _o(e, t) : lo(e, t))),
    fo = a(2, (e, t) => (so(e) ? vo(e, t) : No(e, t))),
    po = a(2, (e, t) => (so(e) ? yo(e, t) : Po(e, t))),
    mo = a(2, (e, t) => {
      if (so(e)) {
        if (e._tag === `Success`) return e;
        let n = da(e.cause);
        return Oi(n) ? e : t(n.success);
      }
      return Mo(e, t);
    }),
    ho = (e) => e._tag === `Success`,
    go = dn(void 0),
    _o = a(2, (e, t) => (e._tag === `Success` ? dn(t(e.value)) : e)),
    vo = a(2, (e, t) => {
      if (e._tag === `Success`) return e;
      let n = da(e.cause);
      return Oi(n) ? e : hn(t(n.success));
    }),
    yo = a(2, (e, t) => {
      if (e._tag === `Success`) return dn(t.onSuccess(e.value));
      let n = da(e.cause);
      return Oi(n) ? e : hn(t.onFailure(n.success));
    }),
    bo = (e) => {
      let t = [];
      for (let n of e) n._tag === `Failure` && t.push(...n.cause.reasons);
      return t.length === 0 ? go : mn(Xt(t));
    },
    xo = a(2, (e, t) =>
      _n((n) => {
        let r = n.context,
          i = t(r);
        return r === i
          ? e
          : (n.setContext(i),
            as(n, () => {
              n.setContext(r);
            }),
            e);
      }),
    ),
    So = () => Co,
    Co = _n((e) => Oa(e.context)),
    wo = (e) => _n((t) => e(t.context)),
    To = a(2, (e, t) => (so(e) ? e : xo(e, Hr(t)))),
    Eo = function () {
      return arguments.length === 1
        ? a(2, (e, t) => Do(e, arguments[0], t))
        : a(3, (e, t, n) => Do(e, t, n)).apply(this, arguments);
    },
    Do = (e, t, n) => xo(e, Nr(t, n)),
    Oo = a(2, (e, t) => new Ao(e, t.length === 1 ? t : (e) => t(e))),
    ko = cn({ op: `OnFailure`, [Nt]: Qa }),
    Ao = function (e, t) {
      ((this[p] = e), (this[Ft] = t));
    };
  Ao.prototype = ko;
  var jo = a(3, (e, t, n) =>
      Oo(e, (e) => {
        let r = t(e);
        return Oi(r) ? ka(r.failure) : n(r.success, e);
      }),
    ),
    Mo = a(2, (e, t) => jo(e, da, (e) => t(e))),
    No = a(2, (e, t) => Mo(e, (e) => za(() => t(e)))),
    Po = a(2, (e, t) =>
      Ro(e, {
        onFailure: (e) => za(() => t.onFailure(e)),
        onSuccess: (e) => ja(() => t.onSuccess(e)),
      }),
    ),
    Fo = a(
      2,
      (e, t) =>
        new Lo(
          e,
          t.onSuccess.length === 1 ? t.onSuccess : (e) => t.onSuccess(e),
          t.onFailure.length === 1 ? t.onFailure : (e) => t.onFailure(e),
        ),
    ),
    Io = cn({ op: `OnSuccessAndFailure`, [Nt]: Qa }),
    Lo = function (e, t, n) {
      ((this[p] = e), (this[Pt] = t), (this[Ft] = n));
    };
  Lo.prototype = Io;
  var Ro = a(2, (e, t) =>
      Fo(e, {
        onFailure: (e) => {
          let n = e.reasons.find(rn);
          return n ? t.onFailure(n.error) : ka(e);
        },
        onSuccess: t.onSuccess,
      }),
    ),
    zo = (e) => (so(e) ? dn(e) : Bo(e)),
    Bo = ln({
      op: `Exit`,
      [Nt](e) {
        return (e._stack.push(this), this[p]);
      },
      [Pt](e, t, n) {
        return t.succeedWith(n ?? dn(e));
      },
      [Ft](e, t, n) {
        return t.succeedWith(n ?? mn(e));
      },
    }),
    Vo = `~effect/Scope`,
    Ho = `~effect/Scope/Closeable`,
    Uo = fr(`effect/Scope`),
    Wo = (e, t) => _n((n) => (ls(n), Go(e, t) ?? Ba)),
    Go = (e, t) => {
      let n = e.state;
      if (
        n._tag === `Closed` ||
        ((e.state = { _tag: `Closed`, exit: t }),
        e.parent !== void 0 && Zo(e.parent, e),
        n._tag === `Empty`)
      )
        return;
      if (n.finalizer !== void 0) return Ko(n.finalizer, t);
      let r = n.finalizers;
      return r.size === 1 ? Ko(r.values().next().value, t) : Jo(e, r, t);
    },
    Ko = (e, t) => {
      try {
        return e(t);
      } catch (e) {
        return gn(e);
      }
    },
    qo = (e, t) => (ho(e) ? t : Oo(t, (t) => ka(Qt(e.cause, t)))),
    Jo = Ka(function* (e, t, n) {
      let r = [],
        i = [],
        a = Array.from(t.values()),
        o = ya();
      for (let t = a.length - 1; t >= 0; t--) {
        let s = a[t];
        e.strategy === `sequential`
          ? r.push(yield* zo(Ko(s, n)))
          : i.push(vs(o, Ko(s, n), !0, !0, `inherit`));
      }
      return (i.length > 0 && (r = yield* Ea(i)), yield* bo(r));
    }),
    Yo = (e, t) => {
      let n = $o(t, e);
      return e.state._tag === `Closed` ? ((n.state = e.state), n) : (Xo(e, n, (e) => Wo(n, e)), n);
    },
    Xo = (e, t, n) => {
      if (e.state._tag === `Empty`)
        e.state = { _tag: `Open`, finalizerKey: t, finalizer: n, finalizers: void 0 };
      else if (e.state._tag === `Open`) {
        let r = e.state;
        r.finalizer === void 0
          ? r.finalizers.set(t, n)
          : ((r.finalizers = new Map([[r.finalizerKey, r.finalizer]])),
            (r.finalizerKey = void 0),
            (r.finalizer = void 0),
            r.finalizers.set(t, n));
      }
    },
    Zo = (e, t) => {
      if (e.state._tag !== `Open`) return;
      let n = e.state;
      n.finalizerKey === t
        ? (e.state = es)
        : n.finalizers !== void 0 &&
          (n.finalizers.delete(t), n.finalizers.size === 0 && (e.state = es));
    },
    Qo = (e) => $o(e, void 0),
    $o = (e = `sequential`, t) => ({ [Ho]: Ho, [Vo]: Vo, strategy: e, parent: t, state: es }),
    es = { _tag: `Empty` },
    ts = Eo(Uo),
    ns = (e) =>
      Ma(() => {
        let t = Qo();
        return os(e(t), (e) => Ma(() => Go(t, e) ?? Ba));
      }),
    rs = (function () {
      let e = cn({
          op: `OnExit`,
          [Nt](e) {
            return (e._stack.push(this), this.effect);
          },
          [It](e) {
            e.interruptible &&
              this.interruptible !== !0 &&
              (e._stack.push(cs), (e.interruptible = !1));
          },
          [Pt](e, t, n) {
            n ??= dn(e);
            let r;
            try {
              r = this.onExit(n);
            } catch (e) {
              r = gn(e);
            }
            return r ? oo(r, (e) => n) : n;
          },
          [Ft](e, t, n) {
            n ??= mn(e);
            let r;
            try {
              r = this.onExit(n);
            } catch (e) {
              r = gn(e);
            }
            return r ? oo(qo(n, r), (e) => n) : n;
          },
        }),
        t = function (e, t, n) {
          ((this.effect = e), (this.onExit = t), (this.interruptible = n));
        };
      return ((t.prototype = e), t);
    })(),
    is = (e, t, n) => new rs(e, t, n),
    as = (e, t) => {
      e._stack.push(new rs(void 0, t, void 0));
    },
    os = a(2, is),
    ss = (e) => _n((t) => (t.interruptible ? ((t.interruptible = !1), t._stack.push(cs), e) : e)),
    cs = ln({
      op: `SetInterruptible`,
      [It](e) {
        if (((e.interruptible = this[p]), e._interruptedCause && e.interruptible))
          return () => ka(e._interruptedCause);
      },
    })(!0),
    ls = (e) => {
      let t = e;
      t.interruptible && ((t.interruptible = !1), t._stack.push(cs));
    },
    us = ln({
      op: `While`,
      [Pt](e, t) {
        return (this[p].step(e), this[p].while() ? (t._stack.push(this), this[p].body()) : go);
      },
      [Nt](e) {
        return this[p].while() ? (e._stack.push(this), this[p].body()) : go;
      },
    }),
    ds = a(
      (e) => typeof e[1] == `function`,
      (e, t, n) =>
        Ma(() => {
          let r = ps(n?.concurrency);
          if (r === 1) return fs(e, t, n);
          let i = Ii(e),
            a = i.length;
          if (a === 0) return n?.discard ? Ba : Oa([]);
          let o = n?.discard ? void 0 : Array(a),
            s = _s({ f: t, out: o }, i, { concurrency: r });
          return s ? Za(s, o) : Oa(o);
        }),
    ),
    fs = (e, t, n) =>
      Ma(() => {
        let r = n?.discard ? void 0 : [],
          i = e[Symbol.iterator](),
          a = i.next(),
          o = 0;
        return Za(
          us({
            while: () => !a.done,
            body: () => t(a.value, o++),
            step: (e) => {
              (r && r.push(e), (a = i.next()));
            },
          }),
          r,
        );
      }),
    ps = (e) => (e === `unbounded` ? 1 / 0 : Math.max(1, e ?? 1)),
    ms = () => (e) => {
      let t = e.onItem,
        n = e.step,
        r = (e, t, r, a, o) => oo(zo(o), (o) => n(e, t[r], o, r) ?? i(e, t, r + 1, a) ?? Ba),
        i = (e, i, a = 0, o = i.length) => {
          for (; a < o; a++) {
            let s = i[a],
              c = t(e, s, a);
            if (!so(c)) return r(e, i, a, o, c);
            let l = n(e, s, c, a);
            if (l) return l._tag === `Failure` ? l : void 0;
          }
        };
      return i;
    },
    hs = (e) => {
      let t = e.onItem,
        n = e.step;
      return (e, r, i) => {
        let a = 0,
          o = i.end ?? r.length,
          s = i.concurrency,
          c = !1,
          l,
          u,
          d,
          f,
          ee,
          te = (e) => {
            let t = gn(e);
            return (
              (f = t), (c = !0), u && u.size > 0 ? oo(ss(Da(Array.from(u))), () => f ?? t) : t
            );
          },
          ne = () => {
            let i = !1;
            for (; !f && a < o; a++) {
              let o = r[a],
                re = ee ?? t(e, o, a);
              if (so(re)) {
                if (((f = n(e, o, re, a)), f)) break;
              } else if (l) {
                ee = void 0;
                let t = vs(l, re, !0, !0, `inherit`);
                if (t._exit) {
                  if (((f = n(e, o, t._exit, a)), f)) break;
                  continue;
                }
                u.add(t);
                let r = a;
                if (
                  (t.addObserver((a) => {
                    u.delete(t);
                    try {
                      if (f) {
                        if (a._tag === `Failure`) {
                          let e = a.cause.reasons.filter((e) => e._tag !== `Interrupt`);
                          if (e.length > 0) {
                            let t = Xt(e);
                            f = mn(f._tag === `Failure` ? Qt(f.cause, t) : t);
                          }
                        }
                      } else {
                        let t = n(e, o, a, r);
                        t && ((f = t), ne());
                      }
                      if (i) {
                        let e = ne();
                        e && d(e);
                      } else c && u.size === 0 && d(f ?? Ba);
                    } catch (e) {
                      d(te(e));
                    }
                  }),
                  u.size < s)
                )
                  continue;
                ((i = !0), a++);
                return;
              } else
                return Wa((e) => {
                  ((l = ya()), (u = new Set()), (ee = re), (d = e));
                  let t;
                  try {
                    t = ne();
                  } catch (t) {
                    return e(te(t));
                  }
                  return t
                    ? e(t)
                    : Ma(
                        () => (
                          (f ??= go),
                          oo(u ? Da(u) : Ba, () => (f?._tag === `Failure` ? f : Ba))
                        ),
                      );
                });
            }
            if (((c = !0), f)) {
              if (u && u.size > 0) {
                let e = Ta(l);
                u.forEach((t) => t.interruptUnsafe(l.id, e));
                return;
              }
              if (d || f._tag === `Failure`) return f;
            } else if (d) {
              if (u) u.size === 0 && d(Ba);
              else return go;
            }
          };
        return ne();
      };
    },
    gs = () => (e) => hs(e),
    _s = hs({
      onItem(e, t, n) {
        return e.f(t, n);
      },
      step(e, t, n, r) {
        if (n._tag === `Failure`) return n;
        e.out && (e.out[r] = n.value);
      },
    }),
    vs = (e, t, n = !1, r = !1, i = !1) => {
      let a = e,
        o = i === `inherit` ? a.interruptible : !i,
        s = new ba(a.context, o);
      return (
        r || (a.children().add(s), (s._parent = a)),
        n
          ? s.evaluate(t)
          : ((s._asyncContext = va()), a.currentDispatcher.scheduleTask(() => s.evaluate(t), 0)),
        s
      );
    },
    ys = (e) => (t, n) => {
      let r = new ba(n?.scheduler ? Nr(e, Ui, n.scheduler) : e, n?.uninterruptible !== !0);
      if ((r.evaluate(t), r._exit)) return r;
      if (n?.signal) {
        if (n.signal.aborted) r.interruptUnsafe();
        else {
          let e = () => r.interruptUnsafe();
          (n.signal.addEventListener(`abort`, e, { once: !0 }),
            r.addObserver(() => n.signal.removeEventListener(`abort`, e)));
        }
      }
      return (n?.onFiberStart && n.onFiberStart(r), r);
    },
    bs = ((e) => {
      let t = ys(e);
      return (e) => {
        if (so(e)) return e;
        let n = new Ji(`sync`),
          r = t(e, { scheduler: n });
        return (r._dispatcher?.flush(), r._exit ?? gn(new ws(r)));
      };
    })(Ar()),
    xs = `~effect/Cause/IllegalArgumentError`,
    Ss = class extends bn(`IllegalArgumentError`) {
      [xs] = xs;
      constructor(e) {
        super({ message: e });
      }
    },
    Cs = `~effect/Cause/AsyncFiberError`,
    ws = class extends bn(`AsyncFiberError`) {
      [Cs] = Cs;
      constructor(e) {
        super({ message: `An asynchronous Effect was executed with Effect.runSync`, fiber: e });
      }
    },
    Ts = `~effect/Cause/UnknownError`,
    Es = class extends bn(`UnknownError`) {
      [Ts] = Ts;
      constructor(e, t) {
        super({ message: t, cause: e });
      }
    },
    Ds = {
      bold: `1`,
      red: `31`,
      green: `32`,
      yellow: `33`,
      blue: `34`,
      cyan: `36`,
      white: `37`,
      gray: `90`,
      black: `30`,
      bgBrightRed: `101`,
    };
  (Ds.gray, Ds.blue, Ds.green, Ds.yellow, Ds.red, Ds.bgBrightRed, Ds.black);
  var Os = dn,
    ks = mn,
    As = hn,
    js = go,
    Ms = ho,
    Ns = {
      "~effect/Deferred": { _A: o, _E: o },
      pipe() {
        return n(this, arguments);
      },
    },
    Ps = function () {
      ((this.resumes = void 0), (this.effect = void 0));
    };
  Ps.prototype = Ns;
  var Fs = () => new Ps(),
    Is = (e) =>
      Wa((t) =>
        e.effect
          ? t(e.effect)
          : ((e.resumes ??= []),
            e.resumes.push(t),
            ja(() => {
              let n = e.resumes;
              if (n === void 0) return;
              let r = n.indexOf(t);
              r >= 0 && n.splice(r, 1);
            })),
      ),
    Ls = (e, t) => {
      if (e.effect) return !1;
      if (((e.effect = t), e.resumes)) {
        let n = e.resumes;
        e.resumes = void 0;
        for (let e = 0; e < n.length; e++) n[e](t);
      }
      return !0;
    },
    Rs = Uo,
    zs = Qo,
    Bs = ts,
    Vs = Yo,
    Hs = Wo,
    Us = `~effect/Layer`,
    Ws = `~effect/Layer/MemoMap`,
    Gs = (e, t) => {
      let n = {
        observers: 0,
        deferred: Fs(),
        scope: zs(),
        finalizer: (r) => Ma(() => (--n.observers > 0 ? Ba : (e.map.delete(t), Hs(n.scope, r)))),
      };
      return n;
    },
    Ks = (e, t) => t.state._tag !== `Closed` && (e.observers++, Xo(t, {}, e.finalizer), !0),
    qs = {
      [Us]: { _ROut: o, _E: o, _RIn: o },
      pipe() {
        return n(this, arguments);
      },
    },
    Js = (e) => {
      let t = Object.create(qs);
      return ((t.build = e), t);
    },
    Ys = (e) =>
      Js((t, n) => {
        let r = Vs(n);
        return os(e(t, r), (e) => (e._tag === `Failure` ? Hs(r, e) : Ba));
      }),
    Xs = (e) => {
      let t = Ys((n, r) => n.getOrElseMemoize(t, r, e));
      return t;
    },
    Zs = class {
      get [Ws]() {
        return Ws;
      }
      parent;
      constructor(e) {
        this.parent = e;
      }
      map = new Map();
      get(e, t) {
        let n = this.map.get(e);
        return n ? (Ks(n, t), n.deferred.effect ?? Is(n.deferred)) : this.parent?.get(e, t);
      }
      getOrElseMemoize(e, t, n) {
        return Ma(() => {
          let r;
          return is(
            Ma(() => {
              let i = this.get(e, t);
              if (i) return i;
              let a = Gs(this, e);
              return Ks(a, t)
                ? ((r = a.deferred), this.map.set(e, a), n(this, a.scope))
                : n(this, t);
            }),
            (e) => {
              r && Ls(r, e);
            },
          );
        });
      }
    },
    Qs = () => new Zs(),
    $s = (e) => new Zs(e),
    ec = class e extends fr()(`effect/Layer/CurrentMemoMap`) {
      static forkOrCreate(t) {
        let n = Fr(t, e);
        return n ? $s(n) : Qs();
      }
    },
    tc = a(3, (e, t, n) =>
      Eo(
        lo(
          Ma(() => e.build(t, n)),
          Nr(ec, t),
        ),
        ec,
        t,
      ),
    ),
    nc = a(2, (e, t) => _n((n) => tc(e, ec.forkOrCreate(n.context), t))),
    rc = (e) => Js(s(Oa(e))),
    ic = (e) => Xs((t, n) => Bs(e, n)),
    ac = (e) => ic(Za(e, Ar())),
    oc = (e, t, n) => {
      let r = Vs(n, `parallel`);
      return ds(e, (e) => e.build(t, Vs(r, `sequential`)), { concurrency: e.length }).pipe(
        lo((e) => Ur(...e)),
      );
    },
    sc = (...e) => Ys((t, n) => oc(e, t, n)),
    cc = (e, t, n) =>
      Ys((r, i) =>
        oo(Array.isArray(t) ? oc(t, r, i) : t.build(r, i), (t) =>
          e.build(r, i).pipe(
            To(t),
            lo((e) => n(e, t)),
          ),
        ),
      ),
    lc = a(2, (e, t) => cc(e, t, o)),
    uc = a(2, (e, t) => cc(e, t, (e, t) => Hr(t, e))),
    dc = rn,
    fc = tn,
    pc = pa,
    mc = Ss,
    hc = `~effect/DateTime`,
    gc = `~effect/DateTime/TimeZone`,
    _c = {
      [hc]: hc,
      pipe() {
        return n(this, arguments);
      },
      [yt]() {
        return this.toString();
      },
      toJSON() {
        return Oc(this).toJSON();
      },
    },
    vc = {
      ..._c,
      _tag: `Utc`,
      [xe]() {
        return Oe(this.epochMilliseconds);
      },
      [Be](e) {
        return bc(e) && e._tag === `Utc` && this.epochMilliseconds === e.epochMilliseconds;
      },
      toString() {
        return `DateTime.Utc(${Oc(this).toJSON()})`;
      },
    };
  ({ ..._c });
  var yc = {
    [gc]: gc,
    [yt]() {
      return this.toString();
    },
  };
  (({ ...yc }), { ...yc });
  var bc = (e) => ye(e, hc),
    xc = (e) => e._tag === `Utc`,
    Sc = (e) => {
      let t = Object.create(vc);
      return (
        (t.epochMilliseconds = e),
        Object.defineProperty(t, "partsUtc", { value: void 0, enumerable: !1, writable: !0 }),
        t
      );
    },
    Cc = (e) => {
      let t = e.getTime();
      if (Number.isNaN(t)) throw new mc(`Invalid date`);
      return Sc(t);
    },
    wc = (e) => {
      if (bc(e)) return e;
      if (e instanceof Date) return Cc(e);
      if (typeof e == `object`) {
        if (`epochMilliseconds` in e) return Cc(new Date(e.epochMilliseconds));
        let t = new Date(0);
        return (kc(t, e), Cc(t));
      }
      return typeof e == `string` && !Tc(e) ? Cc(new Date(e + `Z`)) : Cc(new Date(e));
    },
    Tc = (e) => /Z|GMT|[+-]\d{2}$|[+-]\d{2}:?\d{2}$|\]$/.test(e),
    Ec = lr(wc),
    Dc = (e) => Sc(e.epochMilliseconds),
    Oc = (e) => new Date(e.epochMilliseconds),
    kc = (e, t) => {
      if (
        ((t.year !== void 0 || t.month !== void 0 || t.day !== void 0) &&
          e.setUTCFullYear(
            t.year ?? e.getUTCFullYear(),
            t.month === void 0 ? e.getUTCMonth() : t.month - 1,
            t.day ?? e.getUTCDate(),
          ),
        t.weekDay !== void 0)
      ) {
        let n = t.weekDay - e.getUTCDay();
        e.setUTCDate(e.getUTCDate() + n);
      }
      (t.hour !== void 0 && e.setUTCHours(t.hour),
        t.minute !== void 0 && e.setUTCMinutes(t.minute),
        t.second !== void 0 && e.setUTCSeconds(t.second),
        t.millisecond !== void 0 && e.setUTCMilliseconds(t.millisecond));
    },
    Ac = (e) => Oc(e).toISOString();
  globalThis.String;
  var jc = (e) => e.toLowerCase(),
    Mc = (e) => e.trim(),
    Nc = (e, t, n) => ns((r) => oo(n?.local ? tc(t, Qs(), r) : nc(t, r), (t) => To(e, t))),
    Pc = a(
      (e) => Bt(e[0]),
      (e, t, n) => (Or(t) ? To(e, t) : Nc(e, Array.isArray(t) ? sc(...t) : t, n)),
    ),
    Fc = Bt,
    m = Oa,
    Ic = Ia,
    Lc = Fa,
    Rc = Ma,
    zc = Ga,
    Bc = Aa,
    Vc = ka,
    Hc = La,
    Uc = Ra,
    Wc = Va,
    Gc = _n,
    Kc = Na,
    qc = oo,
    Jc = zo,
    Yc = lo,
    Xc = Oo,
    Zc = No,
    Qc = So,
    $c = wo,
    el = Pc,
    tl = To,
    nl = bs,
    rl = uo,
    il = fo,
    al = po,
    ol = co,
    sl = mo,
    cl = Ja,
    ll = `~effect/BigDecimal`,
    ul = {
      [ll]: ll,
      [xe]() {
        let e = vl(this);
        return Te(ke(String(e.value)), Oe(e.scale));
      },
      [Be](e) {
        return dl(e) && Cl(this, e) === 0;
      },
      toString() {
        return `BigDecimal(${Tl(this)})`;
      },
      toJSON() {
        return { _id: `BigDecimal`, value: String(this.value), scale: this.scale };
      },
      [yt]() {
        return this.toJSON();
      },
      pipe() {
        return n(this, arguments);
      },
    },
    dl = (e) => ye(e, ll),
    fl = (e, t) => {
      if (!Number.isSafeInteger(t)) throw RangeError(`Scale must be a safe integer, got ${t}`);
      let n = Object.create(ul);
      return ((n.value = e), (n.scale = t), n);
    },
    pl = (e, t) => {
      let n = fl(e, t);
      return ((n.normalized = n), n);
    },
    ml = BigInt(0),
    hl = BigInt(1),
    gl = BigInt(10),
    _l = pl(ml, 0),
    vl = (e) => {
      if (e.normalized === void 0) {
        if (e.value === ml) e.normalized = _l;
        else {
          let t = `${e.value}`,
            n = t.length;
          for (; t[n - 1] === `0`;) n--;
          e.normalized = pl(BigInt(t.slice(0, n)), e.scale - (t.length - n));
        }
      }
      return e.normalized;
    },
    yl = 100,
    bl = [hl],
    xl = (e, t) => (e === t ? 0 : e < t ? -1 : 1),
    Sl = (e, t) => {
      let n = `${e.value < ml ? -e.value : e.value}`,
        r = `${t.value < ml ? -t.value : t.value}`,
        i = BigInt(n.length - r.length) - BigInt(e.scale) + BigInt(t.scale);
      if (i !== ml) return i < ml ? -1 : 1;
      let a = Math.max(n.length, r.length);
      return Jn(n.padEnd(a, `0`), r.padEnd(a, `0`));
    },
    Cl = (e, t) => {
      if (e.scale === t.scale) return xl(e.value, t.value);
      let n = wl(e),
        r = wl(t);
      if (n !== r) return n < r ? -1 : 1;
      if (n === 0) return 0;
      let i = e.scale - t.scale,
        a = Math.abs(i);
      if (a > yl) return n === -1 ? Sl(t, e) : Sl(e, t);
      let o = (bl[a] ??= gl ** BigInt(a));
      return i > 0 ? xl(e.value, t.value * o) : xl(e.value * o, t.value);
    },
    wl = (e) => (e.value === ml ? 0 : e.value < ml ? -1 : 1),
    Tl = (e) => {
      let t = vl(e);
      if (Math.abs(t.scale) >= 16) return El(t);
      let n = t.value < ml,
        r = `${n ? -t.value : t.value}`,
        i = t.scale > 0 ? r.padStart(t.scale + 1, `0`) : r.padEnd(r.length - t.scale, `0`),
        a = i.length - t.scale,
        o = t.scale > 0 ? `${i.slice(0, a)}.${i.slice(a)}` : i;
      return n ? `-${o}` : o;
    },
    El = (e) => {
      if (Dl(e)) return `0e+0`;
      let t = vl(e),
        n = `${t.value}`,
        r = t.value < ml ? 2 : 1,
        i = n.slice(0, r),
        a = n.slice(r),
        o = a.length - t.scale;
      return `${i}${a === `` ? `` : `.${a}`}e${o >= 0 ? `+` : ``}${o}`;
    },
    Dl = (e) => e.value === ml,
    Ol = BigInt(1),
    kl = BigInt(1e3),
    Al = BigInt(1024),
    jl = [
      { symbol: `B`, factor: Ol, names: [`B`, `byte`, `bytes`] },
      { symbol: `kB`, factor: kl, names: [`kB`, `kilobyte`, `kilobytes`] },
      { symbol: `MB`, factor: kl ** BigInt(2), names: [`MB`, `megabyte`, `megabytes`] },
      { symbol: `GB`, factor: kl ** BigInt(3), names: [`GB`, `gigabyte`, `gigabytes`] },
      { symbol: `TB`, factor: kl ** BigInt(4), names: [`TB`, `terabyte`, `terabytes`] },
      { symbol: `PB`, factor: kl ** BigInt(5), names: [`PB`, `petabyte`, `petabytes`] },
      { symbol: `EB`, factor: kl ** BigInt(6), names: [`EB`, `exabyte`, `exabytes`] },
      { symbol: `ZB`, factor: kl ** BigInt(7), names: [`ZB`, `zettabyte`, `zettabytes`] },
      { symbol: `YB`, factor: kl ** BigInt(8), names: [`YB`, `yottabyte`, `yottabytes`] },
      { symbol: `RB`, factor: kl ** BigInt(9), names: [`RB`, `ronnabyte`, `ronnabytes`] },
      { symbol: `QB`, factor: kl ** BigInt(10), names: [`QB`, `quettabyte`, `quettabytes`] },
    ],
    Ml = [
      jl[0],
      { symbol: `KiB`, factor: Al, names: [`KiB`, `kibibyte`, `kibibytes`] },
      { symbol: `MiB`, factor: Al ** BigInt(2), names: [`MiB`, `mebibyte`, `mebibytes`] },
      { symbol: `GiB`, factor: Al ** BigInt(3), names: [`GiB`, `gibibyte`, `gibibytes`] },
      { symbol: `TiB`, factor: Al ** BigInt(4), names: [`TiB`, `tebibyte`, `tebibytes`] },
      { symbol: `PiB`, factor: Al ** BigInt(5), names: [`PiB`, `pebibyte`, `pebibytes`] },
      { symbol: `EiB`, factor: Al ** BigInt(6), names: [`EiB`, `exbibyte`, `exbibytes`] },
      { symbol: `ZiB`, factor: Al ** BigInt(7), names: [`ZiB`, `zebibyte`, `zebibytes`] },
      { symbol: `YiB`, factor: Al ** BigInt(8), names: [`YiB`, `yobibyte`, `yobibytes`] },
    ];
  [...jl, ...Ml.slice(1)];
  var Nl = bc,
    Pl = xc,
    Fl = wc,
    Il = Ec,
    Ll = Dc,
    Rl = Ac,
    zl = (e) => Vl(typeof e == `string` ? Hl.encode(e) : e),
    Bl = (e) => {
      let t = Ul(e),
        n = t.length;
      if (n % 4 != 0)
        return Di(
          new ta({
            kind: `Decode`,
            module: `Base64`,
            input: t,
            message: `Length must be a multiple of 4, but is ${n}`,
          }),
        );
      let r = t.indexOf(`=`);
      if (r !== -1 && (r < n - 2 || (r === n - 2 && t[n - 1] !== `=`)))
        return Di(
          new ta({
            kind: `Decode`,
            module: `Base64`,
            input: t,
            message: `Found a '=' character, but it is not at the end`,
          }),
        );
      try {
        let e = t.endsWith(`==`) ? 2 : +!!t.endsWith(`=`),
          r = new Uint8Array((n / 4) * 3 - e);
        for (let e = 0, i = 0; e < n; e += 4, i += 3) {
          let n =
            (Kl(t.charCodeAt(e)) << 18) |
            (Kl(t.charCodeAt(e + 1)) << 12) |
            (Kl(t.charCodeAt(e + 2)) << 6) |
            Kl(t.charCodeAt(e + 3));
          ((r[i] = n >> 16), (r[i + 1] = (n >> 8) & 255), (r[i + 2] = n & 255));
        }
        return Ei(r);
      } catch (e) {
        return Di(
          new ta({
            kind: `Decode`,
            module: `Base64`,
            input: t,
            message: e instanceof Error ? e.message : `Invalid input`,
          }),
        );
      }
    },
    Vl = (e) => {
      let t = e.length,
        n = ``,
        r = 2;
      for (; r < t; r += 3)
        ((n += Wl[e[r - 2] >> 2]),
          (n += Wl[((e[r - 2] & 3) << 4) | (e[r - 1] >> 4)]),
          (n += Wl[((e[r - 1] & 15) << 2) | (e[r] >> 6)]),
          (n += Wl[e[r] & 63]));
      return (
        r === t + 1 && ((n += Wl[e[r - 2] >> 2]), (n += Wl[(e[r - 2] & 3) << 4]), (n += `==`)),
        r === t &&
          ((n += Wl[e[r - 2] >> 2]),
          (n += Wl[((e[r - 2] & 3) << 4) | (e[r - 1] >> 4)]),
          (n += Wl[(e[r - 1] & 15) << 2]),
          (n += `=`)),
        n
      );
    },
    Hl = new TextEncoder(),
    Ul = (e) => e.replace(/[\n\r]/g, ``),
    Wl = `ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/`,
    Gl = new Uint8Array(123).fill(255);
  for (let e = 0; e < 64; e++) Gl[Wl.charCodeAt(e)] = e;
  Gl[61] = 0;
  var Kl = (e) => {
      if (e >= Gl.length || Gl[e] === 255)
        throw TypeError(`Invalid character ${String.fromCharCode(e)}`);
      return Gl[e];
    },
    ql = `~effect/http/Cookies`,
    Jl = {
      [ql]: ql,
      ...xt,
      toJSON() {
        return { _id: `effect/Cookies`, cookies: ji(this.cookies, (e) => e.toJSON()) };
      },
      pipe() {
        return n(this, arguments);
      },
    },
    Yl = (e) => {
      let t = Object.create(Jl);
      return ((t.cookies = e), t);
    },
    Xl = ((e) => {
      let t = {};
      for (let n of e) Dt(t, n.name, n);
      return Yl(t);
    })([]);
  ({ ...xt });
  var Zl = new WeakMap(),
    Ql = `~effect/Redacted`,
    $l = (e) => ye(e, Ql),
    eu = (e, t) => {
      let n = Object.create(tu);
      return (t?.label && (n.label = t.label), Zl.set(n, e), n);
    },
    tu = {
      [Ql]: { _A: (e) => e },
      label: void 0,
      ...Rt,
      toJSON() {
        return this.toString();
      },
      toString() {
        return `<redacted${oe(this.label) ? `:` + this.label : ``}>`;
      },
      [xe]() {
        return Se(Zl.get(this));
      },
      [Be](e) {
        return $l(e) && Ve(Zl.get(this), Zl.get(e));
      },
    },
    nu = Symbol.for(`~effect/http/Headers`),
    ru = Object.defineProperties(Object.create(null), {
      [nu]: { value: nu },
      [nt]: {
        value(e) {
          return cu(this, Rr(e, lu));
        },
      },
      toJSON: {
        value() {
          return it(this);
        },
      },
      [Be]: {
        value(e) {
          return iu(this, e);
        },
      },
      [xe]: {
        value() {
          return je(this);
        },
      },
      toString: { value: xt.toString },
      [yt]: { value: xt[yt] },
    }),
    iu = Pi(wn()),
    au = Object.create(ru),
    ou = (e) => {
      if (e === void 0) return au;
      if (Symbol.iterator in e) {
        let t = Object.create(ru);
        for (let [n, r] of e) t[n.toLowerCase()] = r;
        return t;
      }
      let t = Object.create(ru);
      for (let [n, r] of Object.entries(e))
        Array.isArray(r)
          ? (t[n.toLowerCase()] = r.join(`, `))
          : r !== void 0 && (t[n.toLowerCase()] = r);
      return t;
    },
    su = (e) => Object.setPrototypeOf(e, ru),
    cu = a(2, (e, t) => {
      let n = { ...e },
        r = (t) => {
          if (typeof t == `string`) {
            let r = t.toLowerCase();
            r in e && (n[r] = eu(e[r]));
          } else for (let r in e) r.search(t) !== -1 && (n[r] = eu(e[r]));
        };
      if (Array.isArray(t)) for (let e = 0; e < t.length; e++) r(t[e]);
      else r(t);
      return n;
    }),
    lu = Wr(`effect/Headers/CurrentRedactedNames`, {
      defaultValue: () => [
        `authorization`,
        `cookie`,
        `set-cookie`,
        `x-api-key`,
        `AWSAccessKeyId`,
        `Signature`,
        `sig`,
        `X-Goog-Signature`,
      ],
    });
  ({ ...Rt });
  function uu(e) {
    return e.checks ? e.checks[e.checks.length - 1].annotations : e.annotations;
  }
  function du(e) {
    return (t) => uu(t)?.[e];
  }
  var fu = `~structural`,
    pu = `~sentinels`,
    mu = `~constructor`,
    hu = f((e) => {
      let t = uu(e)?.identifier;
      return typeof t == `string` ? t : e.getExpected(hu);
    }),
    gu = Symbol(),
    _u = Os,
    vu = _u(gu),
    yu = _u(gu),
    bu = (e) => (e === gu ? er() : tr(e)),
    xu = (e) => (e._tag === `None` ? vu : _u(e.value)),
    Su = `~effect/SchemaIssue/Issue`;
  function Cu(e) {
    return ye(e, Su) && e[Su] === Su;
  }
  function wu(e) {
    return Object.hasOwn(e, `input`);
  }
  var Tu = class {
      [Su] = Su;
      constructor(e, t) {
        t?.reportInput === !0 && e !== gu && (this.input = e);
      }
    },
    Eu = class extends Tu {
      _tag = `Filter`;
      filter;
      issue;
      constructor(e, t, n, r) {
        (super(n, r), (this.filter = e), (this.issue = t));
      }
    },
    Du = class extends Tu {
      _tag = `Encoding`;
      ast;
      issue;
      constructor(e, t, n, r) {
        (super(n, r), (this.ast = e), (this.issue = t));
      }
    },
    Ou = class extends Tu {
      _tag = `Pointer`;
      path;
      issue;
      constructor(e, t) {
        (super(), (this.path = e), (this.issue = t));
      }
    },
    ku = class extends Tu {
      _tag = `MissingKey`;
      annotations;
      constructor(e) {
        (super(), (this.annotations = e));
      }
    },
    Au = class extends Tu {
      _tag = `UnexpectedKey`;
      ast;
      constructor(e, t, n) {
        (super(t, n), (this.ast = e));
      }
    },
    ju = class extends Tu {
      _tag = `Composite`;
      ast;
      issues;
      constructor(e, t, n, r) {
        (super(n, r), (this.ast = e), (this.issues = t));
      }
    },
    Mu = class extends Tu {
      _tag = `InvalidType`;
      ast;
      constructor(e, t, n) {
        (super(t, n), (this.ast = e));
      }
    },
    Nu = class extends Tu {
      _tag = `InvalidValue`;
      annotations;
      constructor(e, t, n) {
        (super(t, n), (this.annotations = e));
      }
    };
  function Pu(e, t, n, r, i) {
    return new ju(e, [new Ou([t], n)], r, i);
  }
  var Fu = class extends Tu {
      _tag = `Forbidden`;
      annotations;
      constructor(e, t, n) {
        (super(t, n), (this.annotations = e));
      }
    },
    Iu = class extends Tu {
      _tag = `AnyOf`;
      ast;
      issues;
      constructor(e, t, n, r) {
        (super(n, r), (this.ast = e), (this.issues = t));
      }
    },
    Lu = class extends Tu {
      _tag = `OneOf`;
      ast;
      successes;
      constructor(e, t, n, r) {
        (super(n, r), (this.ast = e), (this.successes = t));
      }
    };
  function Ru(e, t, n) {
    if (Cu(e)) return e;
    if (typeof e == `string`) return new Nu({ message: e }, t, n);
    let r = typeof e.issue == `string` ? new Nu({ message: e.issue }, t, n) : e.issue;
    return new Ou(e.path, r);
  }
  function zu(e, t, n) {
    if (e !== void 0)
      return typeof e == `boolean` ? (e ? void 0 : new Nu(void 0, t, n)) : Ru(e, t, n);
  }
  function Bu(e, t, n, r) {
    return Array.isArray(t)
      ? Bi(t)
        ? t.length === 1
          ? Ru(t[0], n, r)
          : new ju(
              e,
              Hi(t, (e) => Ru(e, n, r)),
              n,
              r,
            )
        : void 0
      : zu(t, n, r);
  }
  var Vu = (e) => {
      let t = Xu(e);
      if (t !== void 0) return t;
      switch (e._tag) {
        case `InvalidType`:
          return Gu(hu(e.ast), e);
        case `InvalidValue`: {
          let t = Wu(e);
          if (t !== void 0) return Gu(t, e);
          let n = Uu(e);
          return n === void 0 ? `Expected a valid value` : `Invalid data ${n}`;
        }
        case `MissingKey`:
          return `Missing key`;
        case `UnexpectedKey`: {
          let t = Uu(e);
          return t === void 0 ? `Expected no excess property` : `Unexpected key with value ${t}`;
        }
        case `Forbidden`:
          return `Forbidden operation`;
        case `OneOf`: {
          let t = Uu(e);
          return t === void 0
            ? `Expected exactly one member to match`
            : `Expected exactly one member to match the input ${t}`;
        }
      }
    },
    Hu = (e) => Xu(e.issue) ?? Xu(e);
  function Uu(e) {
    return wu(e) ? lt(e.input) : void 0;
  }
  function Wu(e) {
    let t = e.annotations?.expected;
    return typeof t == `string` ? t : void 0;
  }
  function Gu(e, t) {
    let n = Uu(t);
    return n === void 0 ? `Expected ${e}` : `Expected ${e}, got ${n}`;
  }
  function Ku(e) {
    let t = e.annotations?.expected;
    if (typeof t == `string`) return t;
    switch (e._tag) {
      case `Filter`:
        return `<filter>`;
      case `FilterGroup`:
        return e.checks.map((e) => Ku(e)).join(` & `);
    }
  }
  function qu() {
    return (e) => Yu(e, ``);
  }
  var Ju = qu();
  function Yu(e, t) {
    let n;
    switch (e._tag) {
      case `Filter`: {
        let r = Hu(e);
        if (r !== void 0) n = r;
        else {
          if (e.issue._tag !== `InvalidValue`) return Yu(e.issue, t);
          let r = Wu(e.issue);
          n = r === void 0 ? Gu(Ku(e.filter), e) : Gu(r, e.issue);
        }
        break;
      }
      case `Encoding`:
        return Yu(e.issue, t);
      case `Pointer`:
        return Yu(e.issue, t + ft(e.path));
      case `Composite`:
      case `AnyOf`:
        if (e._tag === `Composite` || e.issues.length > 0)
          return e.issues.map((e) => Yu(e, t)).join(`
`);
        n = Xu(e) ?? Gu(hu(e.ast), e);
        break;
      default:
        n = Vu(e);
    }
    return t ? `${n}\n  at ${t}` : n;
  }
  function Xu(e) {
    if (e._tag === `Pointer`) return;
    if (e._tag === `Encoding`) return Xu(e.issue);
    let t = (
      e._tag === `Filter`
        ? e.filter.annotations
        : `annotations` in e
          ? e.annotations
          : e.ast.annotations
    )?.[
      e._tag === `MissingKey`
        ? `messageMissingKey`
        : e._tag === `UnexpectedKey`
          ? `messageUnexpectedKey`
          : `message`
    ];
    if (typeof t == `string`) return t;
  }
  function Zu(e) {
    let t;
    for (let n of e.reasons) {
      if (!dc(n) || !Cu(n.error)) return;
      t ??= n.error;
    }
    return t;
  }
  function Qu(e, t) {
    let n = Zu(e);
    if (n === void 0) throw Error(t, { cause: e });
    return n;
  }
  var $u = (e) => Object.assign(Object.create(r), e);
  function ed(e) {
    return cd((t, n) => Bc(e(t, n)));
  }
  function td(e) {
    return ed((t, n) => {
      let r = { message: e(t) };
      return ir(t) ? new Fu(r, t.value, n) : new Fu(r);
    });
  }
  var nd = td(() => `Encoding is not supported`),
    rd = $u({ _tag: `Passthrough` });
  function id() {
    return rd;
  }
  function ad(e) {
    return $u({ _tag: `Transform`, transform: e });
  }
  function od(e) {
    return $u({ _tag: `TransformEffect`, transform: e });
  }
  function sd(e) {
    return $u({ _tag: `TransformOptional`, transform: e });
  }
  function cd(e) {
    return $u({ _tag: `TransformOptionalEffect`, transform: e });
  }
  function ld() {
    return sd(() => er());
  }
  function ud(e) {
    return cd((t) => {
      let n = ur(t, pe);
      return ir(n) ? m(n) : rl(e, tr);
    });
  }
  function dd() {
    return ad(globalThis.String);
  }
  function fd() {
    return ad(globalThis.Number);
  }
  function pd() {
    return ad(globalThis.BigInt);
  }
  function md() {
    return ad(Mc);
  }
  function hd() {
    return ad(jc);
  }
  function gd(e) {
    return od((t, n) =>
      Wc({
        try: () => JSON.parse(t, e?.reviver),
        catch: () => new Nu({ expected: `a valid JSON string` }, t, n),
      }),
    );
  }
  function _d(e) {
    return od((t, n) =>
      Wc({
        try: () => {
          let n = JSON.stringify(t, e?.replacer, e?.space);
          if (n === void 0) throw TypeError(`Value cannot be represented as JSON`);
          return n;
        },
        catch: () => new Nu({ expected: `a JSON-serializable value` }, t, n),
      }),
    );
  }
  function vd() {
    return ad(zl);
  }
  function yd() {
    return od((e, t) => il(Kc(Bl(e)), () => new Nu({ expected: `a valid Base64 string` }, e, t)));
  }
  var bd = class extends i {
      _tag = `Middleware`;
      decode;
      encode;
      constructor(e, t) {
        (super(), (this.decode = e), (this.encode = t));
      }
      flip() {
        return new bd(this.encode, this.decode);
      }
    },
    xd = `~effect/SchemaTransformation/Transformation`,
    Sd = class extends i {
      [xd] = xd;
      _tag = `Transformation`;
      decode;
      encode;
      constructor(e, t) {
        (super(), (this.decode = e), (this.encode = t));
      }
      flip() {
        return new Sd(this.encode, this.decode);
      }
    };
  function Cd(e) {
    return ye(e, xd) && e[xd] === xd;
  }
  var wd = (e) => (Cd(e) ? e : new Sd(e.decode, e.encode));
  function Td(e) {
    return new Sd(od(e.decode), od(e.encode));
  }
  function Ed(e) {
    return new Sd(ad(e.decode), ad(e.encode));
  }
  function Dd(e) {
    return new Sd(sd(e.decode), sd(e.encode));
  }
  function Od() {
    return new Sd(md(), id());
  }
  function kd() {
    return new Sd(hd(), id());
  }
  var Ad = new Sd(id(), id());
  function jd() {
    return Ad;
  }
  var Md = new Sd(fd(), dd()),
    Nd = new Sd(pd(), dd()),
    Pd = (e) => _e(e) && typeof e.message == `string`,
    Fd = (e) => {
      let t = Object.hasOwn(e, `cause`)
        ? Error(e.message, { cause: zd(e.cause) })
        : Error(e.message);
      return (
        typeof e.name == `string` && e.name !== `Error` && (t.name = e.name),
        typeof e.stack == `string` && (t.stack = e.stack),
        t
      );
    },
    Id = (e) => {
      try {
        let t = gt(e);
        return t === void 0 ? lt(e) : JSON.parse(t);
      } catch {
        return lt(e);
      }
    },
    Ld = (e, t, n) => {
      let r = { name: e.name, message: typeof e.message == `string` ? e.message : `` };
      return (
        t?.includeStack && typeof e.stack == `string` && (r.stack = e.stack),
        !t?.excludeCause && e.cause !== void 0 && (r.cause = n(e.cause)),
        r
      );
    },
    Rd = (e) => {
      let t = new WeakSet(),
        n = (r) => {
          if (be(r)) {
            if (t.has(r)) return `[Circular]`;
            t.add(r);
            let i = Ld(r, e, n);
            return (t.delete(r), i);
          }
          return Id(r);
        };
      return n;
    },
    zd = (e) => (Pd(e) ? Fd(e) : e),
    Bd = (e) => Ed({ decode: zd, encode: Rd(e) });
  function Vd() {
    return Ed({ decode: sr, encode: cr });
  }
  var Hd = new Sd(yd(), vd());
  function Ud(e) {
    return new Sd(gd(e ?? {}), _d(e));
  }
  function Wd(e) {
    return (t) => t._tag === e;
  }
  var Gd = Wd(`Declaration`),
    Kd = Wd(`Void`),
    qd = Wd(`Never`),
    Jd = Wd(`Literal`),
    Yd = Wd(`UniqueSymbol`),
    Xd = Wd(`Arrays`),
    Zd = Wd(`Objects`),
    Qd = Wd(`Union`),
    $d = Wd(`Suspend`),
    ef = class {
      to;
      transformation;
      constructor(e, t) {
        ((this.to = e), (this.transformation = t));
      }
    },
    tf = {},
    nf = class {
      isOptional;
      isMutable;
      constructorDefault;
      annotations;
      constructor(e, t, n = void 0, r = void 0) {
        ((this.isOptional = e),
          (this.isMutable = t),
          (this.constructorDefault = n),
          (this.annotations = r));
      }
    },
    rf = `~effect/Schema`,
    af = class {
      [rf] = rf;
      annotations;
      checks;
      encoding;
      context;
      constructor(e = void 0, t = void 0, n = void 0, r = void 0) {
        ((this.annotations = e), (this.checks = t), (this.encoding = n), (this.context = r));
      }
      toString() {
        return `<${this._tag}>`;
      }
    },
    of = class extends af {
      _tag = `Declaration`;
      typeParameters;
      run;
      encodingChecks;
      encodingRun;
      constructor(e, t, n, r, i, a, o, s) {
        (super(n, r, i, a),
          (this.typeParameters = e),
          (this.run = t),
          (this.encodingChecks = o),
          (this.encodingRun = s));
      }
      getParser() {
        let e;
        return (t, n) => (t === gu ? vu : (e ??= this.run(this.typeParameters))(t, this, n));
      }
      _rebuild(e, t, n, r, i) {
        let a = Lp(this.typeParameters, e);
        return a === this.typeParameters &&
          t === this.checks &&
          n === this.encodingChecks &&
          r === this.run &&
          i === this.encodingRun
          ? this
          : new of(a, r, this.annotations, t, void 0, this.context, n, i);
      }
      recur(e) {
        return this._rebuild(e, this.checks, this.encodingChecks, this.run, this.encodingRun);
      }
      flip(e) {
        return this._rebuild(
          e,
          this.encodingChecks,
          this.checks,
          this.encodingRun ?? this.run,
          this.run,
        );
      }
      getExpected() {
        let e = this.annotations?.expected;
        return typeof e == `string` ? e : `<Declaration>`;
      }
    },
    sf = new (class extends af {
      _tag = `Null`;
      getParser() {
        return tm(this, null);
      }
      getExpected() {
        return `null`;
      }
    })(),
    cf = class extends af {
      _tag = `Undefined`;
      getParser() {
        return tm(this, void 0);
      }
      toCodecJson() {
        return Cp(this, [lf]);
      }
      getExpected() {
        return `undefined`;
      }
    },
    lf = new ef(
      sf,
      new Sd(
        ad(() => void 0),
        ad(() => null),
      ),
    ),
    uf = new cf(),
    df = new (class extends af {
      _tag = `Void`;
      getParser() {
        let e = _u(void 0);
        return (t) => (t === gu ? vu : e);
      }
      toCodecJson() {
        return Cp(this, [lf]);
      }
      getExpected() {
        return `void`;
      }
    })(),
    ff = new (class extends af {
      _tag = `Never`;
      getParser() {
        return nm(this, he);
      }
      getExpected() {
        return `never`;
      }
    })(),
    pf = new (class extends af {
      _tag = `Unknown`;
      getParser() {
        return nm(this, ge);
      }
      getExpected() {
        return `unknown`;
      }
    })();
  function mf(e) {
    return typeof e == `number` ? `parts[${e}]` : e;
  }
  function hf(e, t, n) {
    if (n.has(e)) return !0;
    if (e.encoding) throw Error(`TemplateLiteral parts cannot have an encoding at ${mf(t)}`);
    let r;
    switch (e._tag) {
      case `String`:
      case `Number`:
      case `BigInt`:
        r = !0;
        break;
      case `Literal`:
        r = !e.checks;
        break;
      case `TemplateLiteral`: {
        let i = mf(t);
        r = !e.checks && e.parts.every((e, t) => hf(e, `${i}.parts[${t}]`, n));
        break;
      }
      case `Union`: {
        let i = mf(t);
        r = !e.checks && e.types.every((e, t) => hf(e, `${i}.types[${t}]`, n));
        break;
      }
      default:
        return !1;
    }
    return (r && n.add(e), r);
  }
  var gf = class extends af {
    _tag = `TemplateLiteral`;
    parts;
    encodedParts;
    literals;
    suffixLengths;
    constructor(e, t, n, r, i) {
      super(t, n, r, i);
      let a = [],
        o = [],
        s = new Set();
      for (let t = 0; t < e.length; t++) {
        let n = e[t];
        if (!hf(n, t, s)) throw Error(`Invalid TemplateLiteral part ${n._tag}`);
        (a.push(n), o.push(n._tag === `Literal` ? globalThis.String(n.literal) : void 0));
      }
      let c = Array(a.length + 1);
      c[a.length] = 0;
      for (let e = a.length - 1; e >= 0; e--) c[e] = c[e + 1] + (o[e]?.length ?? 0);
      ((this.parts = e), (this.encodedParts = a), (this.literals = o), (this.suffixLengths = c));
    }
    getParser(e) {
      let t = e(Up(bf, new Ef(!1, this.parts.map(om), []), _f(this)));
      return (e, n) => {
        if (e === gu) return vu;
        let r = t(e, n);
        return r._tag === `Success`
          ? yu
          : al(r, { onSuccess: () => e, onFailure: (t) => new ju(this, [t], e, n) });
      };
    }
    getExpected() {
      return `string`;
    }
    matchPart(e, t) {
      return rm(this, e, t) === void 0 ? void 0 : e;
    }
  };
  function _f(e) {
    return new Sd(
      od((t, n) => {
        let r = rm(e, t, n);
        return r
          ? m(r)
          : Bc(new Nu({ expected: `a string matching template literal parts` }, t, n));
      }),
      ad((e) => e.join(``)),
    );
  }
  var vf = class extends af {
    _tag = `Literal`;
    literal;
    constructor(e, t, n, r, i) {
      if ((super(t, n, r, i), typeof e == `number` && !globalThis.Number.isFinite(e)))
        throw Error(`A numeric literal must be finite, got ${lt(e)}`);
      this.literal = e;
    }
    getParser() {
      return tm(this, this.literal);
    }
    matchPart(e, t) {
      return e === globalThis.String(this.literal) ? this.literal : void 0;
    }
    toCodecJson() {
      return typeof this.literal == `bigint` ? yf(this) : this;
    }
    toCodecStringTree() {
      return typeof this.literal == `string` ? this : yf(this);
    }
    getExpected() {
      return typeof this.literal == `string`
        ? JSON.stringify(this.literal)
        : globalThis.String(this.literal);
    }
  };
  function yf(e) {
    let t = globalThis.String(e.literal);
    return Cp(e, [
      new ef(
        new vf(t),
        new Sd(
          ad(() => e.literal),
          ad(() => t),
        ),
      ),
    ]);
  }
  var bf = new (class extends af {
      _tag = `String`;
      getParser() {
        return nm(this, oe);
      }
      matchPart(e, t) {
        let n = this.checks;
        return n && !t.disableChecks && gm(n, e, void 0, this, t) ? void 0 : e;
      }
      getExpected() {
        return `string`;
      }
    })(),
    xf = class extends af {
      _tag = `Number`;
      getParser() {
        return nm(this, se);
      }
      matchKey(e, t) {
        return this._match(cm, e, t);
      }
      matchPart(e, t) {
        return this._match(sm, e, t);
      }
      _match(e, t, n) {
        if (!e.test(t)) return;
        let r = globalThis.Number(t);
        return n.disableChecks || !this.checks
          ? r
          : gm(this.checks, r, void 0, this, n)
            ? void 0
            : r;
      }
      toCodecJson() {
        return this.checks &&
          (Sf(this.checks, `effect/schema/isFinite`) || Sf(this.checks, `effect/schema/isInt`))
          ? this
          : Cp(this, [vp]);
      }
      toCodecStringTree() {
        return this.toCodecJson() === this ? Cp(this, [dm]) : Cp(this, [fm]);
      }
      getExpected() {
        return `number`;
      }
    };
  function Sf(e, t) {
    return e.some(
      (e) =>
        e.annotations?.representation?.id === t || (e._tag === `FilterGroup` && Sf(e.checks, t)),
    );
  }
  var Cf = new xf(),
    wf = new (class extends af {
      _tag = `Boolean`;
      getParser() {
        return nm(this, ce);
      }
      getExpected() {
        return `boolean`;
      }
    })(),
    Tf = new (class extends af {
      _tag = `BigInt`;
      getParser() {
        return nm(this, le);
      }
      matchPart(e, t) {
        if (!pm.test(e)) return;
        let n = globalThis.BigInt(e);
        return t.disableChecks || !this.checks
          ? n
          : gm(this.checks, n, void 0, this, t)
            ? void 0
            : n;
      }
      toCodecStringTree() {
        return Cp(this, [hm]);
      }
      getExpected() {
        return `bigint`;
      }
    })(),
    Ef = class extends af {
      _tag = `Arrays`;
      isMutable;
      elements;
      rest;
      encodingChecks;
      constructor(e, t, n, r, i, a, o, s) {
        (super(r, i, a, o),
          (this.isMutable = e),
          (this.elements = t),
          (this.rest = n),
          (this.encodingChecks = s));
        let c = !1;
        for (let e = 0; e < t.length; e++)
          if (Kp(t[e])) c = !0;
          else if (c) throw Error(`A required element cannot follow an optional element. ts(1257)`);
        if (c && n.length > 1)
          throw Error(`A required element cannot follow an optional element. ts(1257)`);
        for (let e = 1; e < n.length; e++)
          if (Kp(n[e])) throw Error(`An optional element cannot follow a rest element. ts(1266)`);
      }
      getParser(e, t = e) {
        let n = this,
          r,
          i,
          a = n.elements.length,
          o = Math.max(0, n.rest.length - 1);
        function s(e, t) {
          return t < a ? r[t] : t >= e ? i[t - e + 1] : i[0];
        }
        let c = (e) => {
            let { input: t, len: r, options: i } = e;
            if (n.rest.length === 0 && r > a)
              for (let o = a; o < r; o++) {
                let r = new Au(n, t[o], i),
                  a = new Ou([o], r);
                if (i.errors === `all`) e.issues ? e.issues.push(a) : (e.issues = [a]);
                else return Bc(new ju(n, [a], t, i));
              }
            return e.issues ? Bc(new ju(n, e.issues, t, i)) : _u(e.output);
          },
          l = (e, t) => {
            let r = e.length,
              i = {
                ast: n,
                getParser: s,
                input: e,
                len: r,
                tailThreshold: Math.max(a, r - o),
                output: new globalThis.Array(r),
                issues: void 0,
                options: t,
              },
              u = n.rest.length === 0 ? a : Math.max(r, a + o),
              d = t.concurrency === void 0 ? 1 : ps(t.concurrency),
              f = d === 1 ? kf(i, e, 0, u) : Af(i, e, { concurrency: d, end: u });
            if (!f) return c(i);
            if (so(f)) return ol(f, () => c(i));
            let ee = !0;
            return Rc(() => (ee ? ((ee = !1), qc(f, () => c(i))) : l(e, t)));
          };
        return (e, a) => {
          if (e === gu) return vu;
          try {
            return Array.isArray(e)
              ? (r ||
                  ((r = n.elements.map((e) => ({ ast: e, parser: t(e) }))),
                  (i = n.rest.map((e) => ({ ast: e, parser: t(e) })))),
                l(e, a))
              : Bc(new Mu(n, e, a));
          } catch (e) {
            return Uc(e);
          }
        };
      }
      _rebuild(e, t, n) {
        let r = Lp(this.elements, e),
          i = Lp(this.rest, e);
        return r === this.elements &&
          i === this.rest &&
          t === this.checks &&
          n === this.encodingChecks
          ? this
          : new Ef(this.isMutable, r, i, this.annotations, t, void 0, this.context, n);
      }
      recur(e) {
        return this._rebuild(e, this.checks, this.encodingChecks);
      }
      flip(e) {
        return this._rebuild(e, this.encodingChecks, this.checks);
      }
      getExpected() {
        return `array`;
      }
    };
  function Df(e, t, n, r) {
    if (n._tag === `Failure`) return jf(e, e.ast, r, n);
    let i = n === yu ? t : n[p];
    if (i !== gu) e.output[r] = i;
    else {
      let t = e.getParser(e.tailThreshold, r);
      if (Kp(t.ast)) return;
      let n = new Ou([r], new ku(t.ast.context?.annotations));
      if (e.options.errors === `all`) e.issues ? e.issues.push(n) : (e.issues = [n]);
      else return As(new ju(e.ast, [n], e.input, e.options));
    }
  }
  var Of = {
      onItem(e, t, n) {
        let r = n < e.len ? t : gu;
        return e.getParser(e.tailThreshold, n).parser(r, e.options);
      },
      step: Df,
    },
    kf = ms()(Of),
    Af = gs()(Of),
    jf = (e, t, n, r) => {
      if (r.cause.reasons.length === 0) return r;
      let i = Zu(r.cause);
      if (i === void 0)
        return ks(pc(r.cause, (r) => new ju(t, [new Ou([n], r)], e.input, e.options)));
      let a = new Ou([n], i);
      if (e.options.errors === `all`) e.issues ? e.issues.push(a) : (e.issues = [a]);
      else return As(new ju(t, [a], e.input, e.options));
    },
    Mf = `[+-]?\\d*\\.?\\d+(?:[Ee][+-]?\\d+)?`;
  function Nf(e, t, n = tf) {
    let r, i;
    function a(t) {
      switch (t._tag) {
        case `String`:
        case `TemplateLiteral`:
          return (r ??= Object.keys(e)).filter((e) => t.matchPart(e, n) !== void 0);
        case `Number`:
          return (r ??= Object.keys(e)).filter((e) => t.matchKey(e, n) !== void 0);
        case `Symbol`:
          return (i ??= Object.getOwnPropertySymbols(e)).filter(
            (r) => Object.prototype.propertyIsEnumerable.call(e, r) && t.matchKey(r, n) !== void 0,
          );
        case `Union`:
          return [...new Set(t.types.flatMap(a))];
        default:
          return [];
      }
    }
    return a(im(Zp(t)));
  }
  var Pf = class {
    name;
    type;
    constructor(e, t) {
      ((this.name = e), (this.type = t));
    }
  };
  function Ff(e) {
    switch (e._tag) {
      case `String`:
      case `Number`:
      case `Symbol`:
      case `TemplateLiteral`:
        return !0;
      case `Union`:
        return e.types.every(Ff);
      default:
        return !1;
    }
  }
  function If(e) {
    let t = Tp(e);
    switch (t._tag) {
      case `String`:
      case `Number`:
      case `Symbol`:
      case `TemplateLiteral`:
        return !0;
      case `Union`:
        return t.types.every(If);
      default:
        return !1;
    }
  }
  function Lf(e) {
    return Ff(e) && If(e);
  }
  var Rf = class {
      parameter;
      type;
      constructor(e, t) {
        if (!Lf(e)) throw Error(`Invalid index signature parameter ${e._tag}`);
        if (((this.parameter = e), (this.type = t), Kp(t) && !em(t)))
          throw Error(
            "Cannot use `Schema.optionalKey` with index signatures, use `Schema.optional` instead.",
          );
      }
    },
    zf = class extends af {
      _tag = `Objects`;
      propertySignatures;
      indexSignatures;
      encodingChecks;
      constructor(e, t, n, r, i, a, o) {
        (super(n, r, i, a),
          (this.propertySignatures = e),
          (this.indexSignatures = t),
          (this.encodingChecks = o));
      }
      getParser(e, t = e) {
        let n = this,
          r = n.propertySignatures.length,
          i = n.indexSignatures.length;
        if (!r && !i) return nm(n, me);
        let a,
          o,
          s = () => (
            a ||
              ((a = n.propertySignatures.map((e) => ({
                parser: t(e.type),
                name: e.name,
                type: e.type,
              }))),
              (o = i
                ? n.indexSignatures.map((n) => ({
                    is: n,
                    parserKey: e(im(n.parameter)),
                    parserValue: t(n.type),
                  }))
                : void 0)),
            a
          ),
          c = () => {
            let e = new Set(
                n.propertySignatures.map((e) =>
                  typeof e.name == `number` ? globalThis.String(e.name) : e.name,
                ),
              ),
              t = (t, i, a, o, s) => {
                if (s._tag === `Failure`) return jf(t, n, i, s) ?? js;
                let c = s === yu ? o : s[p];
                if (a !== gu && c !== gu) {
                  if (r && (e.has(i) || e.has(typeof a == `number` ? globalThis.String(a) : a)))
                    return js;
                  Dt(t.out, a, c);
                }
                return js;
              },
              c = (e, r, i, a) => {
                if (!a) {
                  let t = i.parserKey(r, e.options);
                  if (!so(t)) return qc(Jc(t), (t) => c(e, r, i, t));
                  a = t;
                }
                if (a._tag === `Failure`) return jf(e, n, r, a) ?? js;
                let o = a === yu ? r : a[p],
                  s = e.input[r],
                  l = i.parserValue(s, e.options);
                return so(l) ? t(e, r, o, s, l) : qc(Jc(l), (n) => t(e, r, o, s, n));
              },
              l = (e, n, r) => {
                let i = e.input[n],
                  a = r.parserValue(i, e.options);
                return so(a) ? t(e, n, n, i, a) : qc(Jc(a), (r) => t(e, n, n, i, r));
              },
              u = i
                ? gs()({
                    onItem: (e, [t, n]) => (n.is.parameter === bf ? l(e, t, n) : c(e, t, n)),
                    step: (e, t, n) => (n._tag === `Failure` ? n : void 0),
                  })
                : void 0;
            return cl(function* (t, d) {
              if (t === gu) return gu;
              if (typeof t != `object` || !t || Array.isArray(t)) return yield* Bc(new Mu(n, t, d));
              s();
              let f = t,
                ee = {},
                te = { ast: n, input: f, out: ee, issues: void 0, options: d },
                ne = d.errors === `all`,
                re = d.onExcessProperty === `error`,
                ie = d.concurrency === void 0 ? 1 : ps(d.concurrency),
                ae = i && re ? n.indexSignatures.map((e) => Nf(f, e.parameter, d)) : void 0;
              if (re) {
                let r = ae ? new Set(e) : e;
                if (ae) for (let e of ae) for (let t of e) r.add(t);
                let i = Reflect.ownKeys(f);
                for (let e = 0; e < i.length; e++) {
                  let a = i[e];
                  if (!r.has(a) && Object.prototype.propertyIsEnumerable.call(f, a)) {
                    let e = new Au(n, f[a], d),
                      r = new Ou([a], e);
                    if (ne) {
                      te.issues ? te.issues.push(r) : (te.issues = [r]);
                      continue;
                    }
                    return yield* Bc(new ju(n, [r], t, d));
                  }
                }
              }
              if (r) {
                let e = ie === 1 ? Hf(te, a) : Uf(te, a, { concurrency: ie });
                e && (yield* e);
              }
              if (i && ie === 1)
                for (let e = 0; e < i; e++) {
                  let t = o[e],
                    n = t.is.parameter === bf ? l : c,
                    r =
                      ae?.[e] ??
                      (t.is.parameter === bf ? Object.keys(f) : Nf(f, t.is.parameter, d));
                  for (let e = 0; e < r.length; e++) {
                    let i = n(te, r[e], t);
                    if (!so(i)) yield* i;
                    else if (i._tag === `Failure`) return yield* i;
                  }
                }
              else if (u) {
                let e = Vi();
                for (let t = 0; t < i; t++) {
                  let n = o[t],
                    r =
                      ae?.[t] ??
                      (n.is.parameter === bf ? Object.keys(f) : Nf(f, n.is.parameter, d));
                  for (let t = 0; t < r.length; t++) e.push([r[t], n]);
                }
                let t = u(te, e, { concurrency: ie });
                t && (yield* t);
              }
              return te.issues ? yield* Bc(new ju(n, te.issues, t, d)) : ee;
            });
          };
        if (i) return c();
        let l,
          u = (e, t, n) => {
            let r = a[t];
            return qc(Jc(n), (n) => {
              let i = Bf(e, r, n);
              if (i) return i;
              let o = () => _u(e.out),
                s = Hf(e, a.slice(t + 1));
              return s ? ol(s, o) : o();
            });
          };
        return (e, t) => {
          if (e === gu) return vu;
          if (
            t.errors === `all` ||
            t.onExcessProperty !== void 0 ||
            (t.concurrency !== void 0 && ps(t.concurrency) !== 1)
          )
            return (l ??= c())(e, t);
          if (typeof e != `object` || !e || Array.isArray(e)) return Bc(new Mu(n, e, t));
          let r = s(),
            i = e,
            a = {},
            o = { ast: n, input: i, out: a, issues: void 0, options: t };
          try {
            for (let e = 0; e < r.length; e++) {
              let n = r[e],
                s = n.name,
                c = tp(i, s),
                l = c ? i[s] : gu,
                d = n.parser(l, t);
              if (!so(d)) return u(o, e, d);
              if (d === yu) {
                c && Dt(a, s, l);
                continue;
              }
              let f = Bf(o, n, d);
              if (f) return f;
            }
          } catch (e) {
            return Uc(e);
          }
          return _u(a);
        };
      }
      _rebuild(e, t, n, r) {
        let i = Lp(this.propertySignatures, (t) => {
            let n = e(t.type);
            return n === t.type ? t : new Pf(t.name, n);
          }),
          a = Lp(this.indexSignatures, (n) => {
            let r = t(n.parameter),
              i = e(n.type);
            return r === n.parameter && i === n.type ? n : new Rf(r, i);
          });
        return i === this.propertySignatures &&
          a === this.indexSignatures &&
          n === this.checks &&
          r === this.encodingChecks
          ? this
          : new zf(i, a, this.annotations, n, void 0, this.context, r);
      }
      flip(e) {
        return this._rebuild(e, e, this.encodingChecks, this.checks);
      }
      recur(e, t = e) {
        return this._rebuild(e, t, this.checks, this.encodingChecks);
      }
      getExpected() {
        return this.propertySignatures.length === 0 && this.indexSignatures.length === 0
          ? `object | array`
          : `object`;
      }
    };
  function Bf(e, t, n) {
    if (n._tag === `Failure`) return jf(e, e.ast, t.name, n);
    if (n === yu) return;
    let r = n[p];
    if (r !== gu) {
      Dt(e.out, t.name, r);
      return;
    }
    if ((delete e.out[t.name], !Kp(t.type))) {
      let n = new Ou([t.name], new ku(t.type.context?.annotations));
      if (e.options.errors === `all`) {
        e.issues ? e.issues.push(n) : (e.issues = [n]);
        return;
      }
      return As(new ju(e.ast, [n], e.input, e.options));
    }
  }
  var Vf = {
      onItem(e, t) {
        if (!tp(e.input, t.name)) return t.parser(gu, e.options);
        let n = e.input[t.name];
        return (Dt(e.out, t.name, n), t.parser(n, e.options));
      },
      step: Bf,
    },
    Hf = ms()(Vf),
    Uf = gs()(Vf);
  function Wf(e, t) {
    return e ? (t ? [...e, ...t] : e) : t;
  }
  function Gf(e, t, n) {
    return new zf(
      Reflect.ownKeys(e).map((t) => new Pf(t, e[t].ast)),
      [],
      n,
      t,
    );
  }
  function Kf(e) {
    return e.ast;
  }
  function qf(e, t, n) {
    return new rp(e.map(Kf), t, void 0, n);
  }
  function Jf(e) {
    if (e.encoding) throw Error(`mutable does not support encodings`);
    return new Ef(
      !0,
      e.elements,
      e.rest,
      e.annotations,
      e.checks,
      void 0,
      e.context,
      e.encodingChecks,
    );
  }
  var Yf = ee((e) => {
    for (;;) {
      if ($d(e)) return pf;
      let t = e.encoding;
      if (!t) return e.recur?.(Yf, o) ?? e;
      if (t.some((e) => e.transformation._tag === `Middleware` && e.transformation.decode !== o))
        return pf;
      e = t[t.length - 1].to;
    }
  });
  function Xf(e) {
    switch (e._tag) {
      case `Null`:
        return [`null`];
      case `Undefined`:
        return [`undefined`];
      case `String`:
      case `TemplateLiteral`:
        return [`string`];
      case `Number`:
        return [`number`];
      case `Boolean`:
        return [`boolean`];
      case `Symbol`:
      case `UniqueSymbol`:
        return [`symbol`];
      case `BigInt`:
        return [`bigint`];
      case `Arrays`:
        return [`array`];
      case `ObjectKeyword`:
        return [`object`, `array`, `function`];
      case `Objects`:
        return e.propertySignatures.length || e.indexSignatures.length
          ? [`object`]
          : [`string`, `number`, `boolean`, `symbol`, `bigint`, `object`, `array`, `function`];
      case `Enum`:
        return Array.from(new Set(e.enums.map(([, e]) => typeof e)));
      case `Literal`:
        return [typeof e.literal];
      case `Union`:
        return Array.from(new Set(e.types.flatMap(Xf)));
      default:
        return [
          `null`,
          `undefined`,
          `string`,
          `number`,
          `boolean`,
          `symbol`,
          `bigint`,
          `object`,
          `array`,
          `function`,
        ];
    }
  }
  function Zf(e) {
    switch (e._tag) {
      default:
        return [];
      case `Declaration`: {
        let t = e.annotations?.[pu];
        return Array.isArray(t) ? t : [];
      }
      case `Objects`:
        return e.propertySignatures.flatMap((e) => {
          let t = e.type;
          if (!Kp(t)) {
            if (Jd(t)) return [{ key: e.name, literal: t.literal }];
            if (Yd(t)) return [{ key: e.name, literal: t.symbol }];
          }
          return [];
        });
      case `Arrays`:
        return e.elements.flatMap((e, t) => {
          if (!Kp(e)) {
            if (Jd(e)) return [{ key: t, literal: e.literal }];
            if (Yd(e)) return [{ key: t, literal: e.symbol }];
          }
          return [];
        });
      case `Union`: {
        if (e.types.length === 0) return [];
        let t = e.types.map((e) => Zf(Yf(e)));
        return t[0].filter((e) =>
          t.every((t) => t.some((t) => t.key === e.key && t.literal === e.literal)),
        );
      }
      case `Suspend`:
        return Zf(e.thunk());
    }
  }
  var Qf = new WeakMap(),
    $f = Object.freeze([]),
    ep = (e) => (e === null ? `null` : Array.isArray(e) ? `array` : typeof e),
    tp = (e, t) => (t === `__proto__` ? Object.hasOwn(e, t) : t in e);
  function np(e) {
    let t = Qf.get(e);
    if (t) return t;
    let n,
      r = 0,
      i,
      a,
      o = !0,
      s = [];
    for (let t = 0; t < e.length; t++) {
      let c = e[t],
        l = Yf(c);
      if (qd(l)) continue;
      if (Jd(l) || Yd(l)) {
        a ??= new Map();
        let e = Jd(l) ? l.literal : l.symbol;
        s[t] = e;
        let n = a.get(e);
        (n || a.set(e, (n = [])), n.push(t));
      } else o = !1;
      let u = Zf(l);
      if (u.length) {
        ((n ??= new Map()), r++);
        for (let { key: e, literal: r } of u) {
          let i = n.get(e);
          (i || n.set(e, (i = [new Map(), new Set()])), i[1].add(t));
          let a = i[0].get(r);
          (a || i[0].set(r, (a = new Set())), a.add(t));
        }
      } else {
        i ??= {};
        let e = Xf(l);
        for (let n of e) (i[n] ??= []).push(t);
      }
    }
    let c = {},
      l = (e) => (c[e] ??= Object.freeze(i?.[e] ?? $f));
    if (o && a) (a.forEach(Object.freeze), (t = (e) => a.get(e) ?? $f));
    else if (n?.size === 1 && !i) {
      let [r, [i]] = n.entries().next().value,
        a = new Map();
      for (let [e, t] of i) a.set(e, Object.freeze(Array.from(t)));
      let o = Object.freeze(e.map((e, t) => t));
      t = (e, t) => {
        if (ve(e)) {
          let n = tp(e, r) ? e[r] : void 0;
          if (n !== void 0) return a.get(n) ?? $f;
          if (t) return o;
        }
        return $f;
      };
    } else if (n) {
      let e;
      for (let t of n) (!e || t[1][0].size > e[1][0].size) && t[1][1].size === r && (e = t);
      t = (t, r) => {
        let a = ep(t);
        if (!ve(t)) return l(a);
        let o = new Set(i?.[a]),
          s;
        if (e) {
          let [n, [i]] = e,
            c = tp(t, n),
            u = c ? t[n] : void 0;
          if (c && (!r || u !== void 0)) {
            let e = i.get(u);
            if (!e) return l(a);
            for (let t of e) o.add(t);
            s = n;
          }
        }
        if (s === void 0)
          for (let [e, [i, a]] of n) {
            let n = tp(t, e),
              s = n ? t[e] : void 0;
            if (n && (!r || s !== void 0)) {
              let e = i.get(s);
              if (e) for (let t of e) o.add(t);
            } else if (r) for (let e of a) o.add(e);
          }
        for (let [e, [i, a]] of n) {
          if (e === s) continue;
          let n = tp(t, e),
            c = n ? t[e] : void 0;
          if (n && (!r || c !== void 0)) {
            let e = i.get(c);
            for (let t of o) a.has(t) && !e?.has(t) && o.delete(t);
          }
        }
        return Array.from(o).sort((e, t) => e - t);
      };
    } else
      t = (e) => {
        let t = l(ep(e));
        return a ? t.filter((t) => s[t] === void 0 || s[t] === e) : t;
      };
    return (Qf.set(e, t), t);
  }
  var rp = class extends af {
    _tag = `Union`;
    types;
    options;
    encodingChecks;
    constructor(e, t, n, r, i, a, o) {
      (super(n, r, i, a), (this.types = e), (this.options = t), (this.encodingChecks = o));
    }
    getParser(e, t) {
      let n = this,
        r = t !== void 0,
        i = [],
        a = (t) => (i[t] ??= e(n.types[t])),
        o;
      return (e, t) => {
        if (e === gu) return vu;
        let i = (o ??= np(n.types))(e, r);
        if (i.length === 0) return Bc(new Iu(n, [], e, t));
        if (i.length === 1) {
          let r = a(i[0])(e, t);
          return r._tag === `Success` ? r : so(r) ? ip(n, r.cause, e, t) : ap(n, r, e, t);
        }
        return op(n, a, i, e, t);
      };
    }
    _rebuild(e, t, n) {
      let r = Lp(this.types, e);
      return r === this.types && t === this.checks && n === this.encodingChecks
        ? this
        : new rp(r, this.options, this.annotations, t, void 0, this.context, n);
    }
    recur(e) {
      return this._rebuild(e, this.checks, this.encodingChecks);
    }
    flip(e) {
      return this._rebuild(e, this.encodingChecks, this.checks);
    }
    matchPart(e, t) {
      for (let n of this.types) {
        let r = n.matchPart(e, t);
        if (r !== void 0) return r;
      }
    }
    getExpected(e) {
      let t = this.annotations?.expected;
      if (typeof t == `string`) return t;
      if (this.types.length === 0) return `never`;
      let n = this.types.map((t) => {
        let n = Zp(t);
        switch (n._tag) {
          case `Arrays`: {
            let t = n.elements.filter(Jd);
            if (t.length > 0)
              return `${up(n.isMutable)}[ ${t.map((t) => e(t) + dp(t.context?.isOptional)).join(`, `)}, ... ]`;
            break;
          }
          case `Objects`: {
            let t = n.propertySignatures.filter((e) => Jd(e.type));
            if (t.length > 0)
              return `{ ${t.map((t) => `${up(t.type.context?.isMutable)}${dt(t.name)}${dp(t.type.context?.isOptional)}: ${e(t.type)}`).join(`, `)}, ... }`;
            break;
          }
        }
        return e(n);
      });
      return Array.from(new Set(n)).join(` | `);
    }
  };
  function ip(e, t, n, r) {
    let i = Zu(t);
    return i ? As(new Iu(e, [i], n, r)) : ks(t);
  }
  function ap(e, t, n, r) {
    return Xc(t, (t) => ip(e, t, n, r));
  }
  function op(e, t, n, r, i) {
    let a = {
        ast: e,
        parser: t,
        input: r,
        out: void 0,
        successes: e.options?.mode === `oneOf` ? [] : void 0,
        issues: void 0,
        options: i,
      },
      o = cp(a, n);
    if (!o) return a.out ? a.out : Bc(new Iu(e, a.issues ?? [], r, i));
    if (so(o)) return sp(o, a);
    let s = !0;
    return Rc(() => (s ? ((s = !1), sp(o, a)) : op(e, t, n, r, i)));
  }
  function sp(e, t) {
    return ol(e, (e) =>
      t.out === yu
        ? m(t.input)
        : t.out
          ? t.out
          : Bc(new Iu(t.ast, t.issues ?? [], t.input, t.options)),
    );
  }
  var cp = ms()({
      onItem(e, t) {
        return e.parser(t)(e.input, e.options);
      },
      step(e, t, n) {
        if (n._tag === `Failure`) {
          let t = Zu(n.cause);
          if (t === void 0) return n;
          e.issues ? e.issues.push(t) : (e.issues = [t]);
        } else {
          if (e.out && e.successes)
            return (
              e.successes.push(e.ast.types[t]), As(new Lu(e.ast, e.successes, e.input, e.options))
            );
          if (((e.out = n), e.successes)) e.successes.push(e.ast.types[t]);
          else return js;
        }
      },
    }),
    lp = new rp([new vf(`Infinity`), new vf(`-Infinity`), new vf(`NaN`)]);
  function up(e) {
    return e ? `` : `readonly `;
  }
  function dp(e) {
    return e ? `?` : ``;
  }
  var fp = class extends af {
      _tag = `Suspend`;
      thunk;
      constructor(e, t, n, r, i) {
        if (n) throw Error(`Cannot add checks to Suspend`);
        super(t, void 0, r, i);
        let a;
        this.thunk = () => (a ??= e());
      }
      getParser(e) {
        let t;
        return (n, r) => (t ??= e(this.thunk()))(n, r);
      }
      recur(e) {
        return new fp(() => e(this.thunk()), this.annotations, void 0, void 0, this.context);
      }
      getExpected(e) {
        return e(this.thunk());
      }
    },
    pp = class extends i {
      _tag = `Filter`;
      run;
      annotations;
      aborted;
      constructor(e, t = void 0, n = !1) {
        (super(), (this.run = e), (this.annotations = t), (this.aborted = n));
      }
      annotate(e) {
        return new pp(this.run, { ...this.annotations, ...e }, this.aborted);
      }
      abort() {
        return new pp(this.run, this.annotations, !0);
      }
      and(e, t) {
        return new mp([this, e], t);
      }
    },
    mp = class extends i {
      _tag = `FilterGroup`;
      checks;
      annotations;
      constructor(e, t = void 0) {
        (super(), (this.checks = e), (this.annotations = t));
      }
      annotate(e) {
        return new mp(this.checks, { ...this.annotations, ...e });
      }
      and(e, t) {
        return new mp([this, e], t);
      }
    };
  function hp(e, t, n = !1) {
    return new pp((t, n, r) => Bu(n, e(t, n, r), t, r), t, n);
  }
  function gp(e) {
    return hp((e) => globalThis.Number.isFinite(e), {
      expected: `a finite number`,
      representation: { id: `effect/schema/isFinite`, payload: null },
      toJsonSchema: () => ({ type: `number` }),
      toCode: () => ({ runtime: `Schema.isFinite()` }),
      arbitraryConstraint: { number: `finite` },
      ...e,
    });
  }
  var _p = Op(Cf, [gp()]),
    vp = new ef(
      new rp([_p, lp]),
      new Sd(
        fd(),
        ad((e) => (globalThis.Number.isFinite(e) ? e : globalThis.String(e))),
      ),
    );
  function yp(e, t) {
    let n = new globalThis.RegExp(e),
      r = { source: n.source, flags: n.flags };
    return hp((e) => ((n.lastIndex = 0), n.test(e)), {
      expected: `a string matching the RegExp ${r.source}`,
      representation: { id: `effect/schema/isPattern`, payload: r },
      toJsonSchema: () => [{}, !0],
      arbitraryConstraint: { patterns: [r] },
      ...t,
    });
  }
  var bp = new WeakMap();
  function xp(e, t) {
    let n = Object.assign(Object.create(Object.getPrototypeOf(e)), e, t);
    return (
      Reflect.ownKeys(t).every((e) => e === `context` || e === `encoding`) && bp.set(n, Sp(e)), n
    );
  }
  function Sp(e) {
    let t = bp.get(e);
    if (t !== void 0) return t;
    if (e.encoding === void 0) return e;
    let n = Object.assign(Object.create(Object.getPrototypeOf(e)), e, { encoding: void 0 });
    return (bp.set(e, n), n);
  }
  function Cp(e, t) {
    return e.encoding === t ? e : xp(e, { encoding: t });
  }
  function wp(e, t) {
    if (e.context === t) return e;
    let n = Sp(e);
    return n.context === t && n.encoding === e.encoding ? n : xp(e, { context: t });
  }
  function Tp(e) {
    return e.encoding ? Tp(e.encoding[e.encoding.length - 1].to) : e;
  }
  function Ep(e, t) {
    if (e.checks) {
      let n = e.checks[e.checks.length - 1];
      return Dp(e, Ri(e.checks.slice(0, -1), n.annotate(t)));
    }
    return xp(e, { annotations: { ...e.annotations, ...t } });
  }
  function Dp(e, t) {
    if (e._tag === `Suspend` && t) throw Error(`Cannot add checks to Suspend`);
    return e.checks === t ? e : xp(e, { checks: t });
  }
  function Op(e, t) {
    return Dp(e, Wf(e.checks, t));
  }
  function kp(e, t) {
    let n = t(e.to);
    return n === e.to ? e : new ef(n, e.transformation);
  }
  function Ap(e, t) {
    let n = e,
      r = n[n.length - 1],
      i = kp(r, t);
    return i === r ? e : Ri(e.slice(0, e.length - 1), i);
  }
  function jp(e) {
    return (t) => (t.encoding ? Cp(t, Ap(t.encoding, e)) : t);
  }
  function Mp(e, t) {
    return jp((e) => wp(e, t))(e);
  }
  function Np(e, t) {
    function n(r) {
      if (r.encoding) {
        let e = r.encoding[r.encoding.length - 1];
        return t?.stopAt?.(e) ? r : Cp(r, Ap(r.encoding, n));
      }
      return e(r);
    }
    return ee(n);
  }
  function Pp(e, t) {
    return Ip(e, t, Xp(e));
  }
  function Fp(e, t) {
    return Ip(Zp(e), t, e);
  }
  function Ip(e, t, n) {
    let r = new ef(e, t);
    return Cp(n, n.encoding ? [...n.encoding, r] : [r]);
  }
  function Lp(e, t) {
    let n;
    for (let r = 0; r < e.length; r++) {
      let i = e[r],
        a = t(i);
      if (n) n[r] = a;
      else if (a !== i) {
        n = Array(e.length);
        for (let t = 0; t < r; t++) n[t] = e[t];
        n[r] = a;
      }
    }
    return n ?? e;
  }
  function Rp(e, t) {
    return wp(
      e,
      e.context
        ? new nf(e.context.isOptional, e.context.isMutable, e.context.constructorDefault, {
            ...e.context.annotations,
            ...t,
          })
        : new nf(!1, !1, void 0, t),
    );
  }
  var zp = ee((e) =>
      Bp(
        wp(
          e,
          e.context
            ? e.context.isOptional === !1
              ? new nf(!0, e.context.isMutable, e.context.constructorDefault, e.context.annotations)
              : e.context
            : new nf(!0, !1),
        ),
      ),
    ),
    Bp = jp(zp),
    Vp = f((e) => zp(new rp([e, uf])));
  function Hp(e, t) {
    return wp(
      e,
      e.context
        ? new nf(e.context.isOptional, e.context.isMutable, t, e.context.annotations)
        : new nf(!1, !1, t),
    );
  }
  function Up(e, t, n) {
    return Ip(e, n, t);
  }
  function Wp(e) {
    let t = [],
      n = [];
    function r(e) {
      switch (e._tag) {
        case `Literal`:
          de(e.literal) && !t.includes(e.literal) && t.push(e.literal);
          return;
        case `UniqueSymbol`:
          t.includes(e.symbol) || t.push(e.symbol);
          return;
        case `Never`:
          return;
        case `Union`:
          for (let t = 0; t < e.types.length; t++) r(e.types[t]);
          return;
        default:
          n.push(e);
      }
    }
    return (r(e), { literals: t, parameters: n });
  }
  function Gp(e, t) {
    let { literals: n, parameters: r } = Wp(e);
    return new zf(
      n.map((e) => new Pf(e, t)),
      r.map((e) => new Rf(e, t)),
    );
  }
  function Kp(e) {
    return e.context?.isOptional ?? !1;
  }
  function qp(e) {
    return (
      e.annotations?.[`~structural`] === !0 || (e._tag === `FilterGroup` && e.checks.every(qp))
    );
  }
  function Jp(e) {
    function t(e) {
      return qp(e) ? [e] : e._tag === `FilterGroup` ? e.checks.flatMap(t) : [];
    }
    let n = e.flatMap(t);
    return zi(n) ? n : void 0;
  }
  function Yp(e) {
    let t = !0;
    function n(e) {
      return ((t = t && !e.encoding && !$d(e)), t && `recur` in e && e.recur(n), e);
    }
    return (`recur` in e && e.recur(n), t);
  }
  var Xp = ee((e) => {
      let t = Sp(e);
      if (t !== e) {
        let n = Xp(t);
        return n === t && e.encoding === void 0 ? e : wp(n, e.context);
      }
      let n = `recur` in e ? e.recur(Xp) : e;
      if (`encodingChecks` in n && n.encodingChecks) {
        let t = Yp(e)
          ? n.encodingChecks
          : Xd(n) || Zd(n) || (Gd(n) && n.typeParameters.length > 0)
            ? Jp(n.encodingChecks)
            : void 0;
        return xp(n, { encodingChecks: void 0, checks: Wf(n.checks, t) });
      }
      return n;
    }),
    Zp = ee((e) => Xp($p(e)));
  function Qp(e, t) {
    let n = t,
      r = n.length,
      i = n[r - 1],
      a = [new ef($p(Cp(e, void 0)), n[0].transformation.flip())];
    for (let e = 1; e < r; e++) a.unshift(new ef($p(n[e - 1].to), n[e].transformation.flip()));
    let o = $p(i.to);
    return o.encoding ? Cp(o, [...o.encoding, ...a]) : Cp(o, a);
  }
  var $p = f((e) => {
    if (e.encoding) return Qp(e, e.encoding);
    let t = Sp(e);
    if (t !== e) {
      let n = $p(t);
      return n === t ? e : wp(n, e.context);
    }
    return `flip` in e ? e.flip($p) : `recur` in e ? e.recur($p) : e;
  });
  function em(e) {
    switch (e._tag) {
      case `Undefined`:
        return !0;
      case `Union`:
        return e.types.some(em);
      default:
        return !1;
    }
  }
  function tm(e, t) {
    let n = t === 0 ? yu : _u(t);
    return (r, i) => (r === gu ? vu : r === t ? n : Bc(new Mu(e, r, i)));
  }
  function nm(e, t) {
    return (n, r) => (n === gu ? vu : t(n) ? yu : Bc(new Mu(e, n, r)));
  }
  function rm(e, t, n) {
    let r = e.encodedParts,
      i = e.literals,
      a = t.length;
    for (let e = 0; e < i.length; e++) {
      let n = i[e];
      if (n && !t.includes(n)) return;
    }
    if (e.suffixLengths[0] > a) return;
    let o = Array(r.length),
      s;
    function c(l, u) {
      if (l === r.length) return u === a;
      if (s?.has(l * (a + 1) + u)) return !1;
      let d = r[l];
      if (l === r.length - 1) {
        let e = t.slice(u);
        if (d.matchPart(e, n) !== void 0) return ((o[l] = e), !0);
      } else if (d._tag === `Literal`) {
        let e = i[l];
        if (t.startsWith(e, u) && c(l + 1, u + e.length)) return ((o[l] = e), !0);
      } else {
        let r = a - e.suffixLengths[l + 1],
          s = i[l + 1],
          f = s === void 0 ? r : t.lastIndexOf(s, r);
        for (; f >= u;) {
          let e = t.slice(u, f);
          if (d.matchPart(e, n) !== void 0 && c(l + 1, f)) return ((o[l] = e), !0);
          if (f === 0) break;
          f = s === void 0 ? f - 1 : t.lastIndexOf(s, f - 1);
        }
      }
      return ((s ??= new Set()), s.add(l * (a + 1) + u), !1);
    }
    return c(0, 0) ? o : void 0;
  }
  var im = Np((e) => {
      switch (e._tag) {
        default:
          return e;
        case `Number`:
          return e.toCodecStringTree();
        case `Union`:
          return e.recur(im);
      }
    }),
    am = Np((e) => {
      switch (e._tag) {
        default:
          return e;
        case `Symbol`:
        case `UniqueSymbol`:
          return e.toCodecStringTree();
        case `Union`:
          return e.recur(am);
      }
    }),
    om = Np((e) => {
      switch (e._tag) {
        default:
          return e;
        case `Number`:
        case `Literal`:
        case `BigInt`:
          return e.toCodecStringTree();
        case `Union`:
          return e.recur(om);
      }
    }),
    sm = new globalThis.RegExp(`^${Mf}$`),
    cm = new globalThis.RegExp(`^(?:${Mf}|Infinity|-Infinity|NaN)$`);
  function lm(e) {
    return yp(sm, {
      expected: `a string representing a finite number`,
      representation: { id: `effect/schema/isStringFinite`, payload: null },
      toJsonSchema: () => ({ pattern: sm.source }),
      ...e,
    });
  }
  var um = Op(bf, [lm()]),
    dm = new ef(um, Md),
    fm = new ef(new rp([um, lp]), Md),
    pm = new globalThis.RegExp(`^-?\\d+$`);
  function mm(e) {
    return yp(pm, {
      expected: `a string representing a bigint`,
      representation: { id: `effect/schema/isStringBigInt`, payload: null },
      toJsonSchema: () => ({ pattern: pm.source }),
      ...e,
    });
  }
  var hm = new ef(Op(bf, [mm({ expected: `a string representing a bigint` })]), Nd);
  function gm(e, t, n, r, i) {
    for (let a = 0; a < e.length; a++) {
      let o = e[a];
      if (o._tag === `FilterGroup`) {
        if (
          ((n = gm(o.checks, t, n, r, i)),
          n && (i.errors !== `all` || n[n.length - 1].filter.aborted))
        )
          return n;
      } else {
        let e = o.run(t, r, i);
        if (e) {
          let r = new Eu(o, e, t, i);
          if ((n ? n.push(r) : (n = [r]), i.errors !== `all` || o.aborted)) return n;
        }
      }
    }
    return n;
  }
  function _m(e) {
    if (!Gd(e)) return;
    let t = e.annotations?.[mu];
    return fe(t) ? t(e.typeParameters) : void 0;
  }
  var vm = du;
  function ym(e) {
    return (
      e === null ||
      typeof e == `string` ||
      typeof e == `boolean` ||
      (typeof e == `number` && globalThis.Number.isFinite(e))
    );
  }
  function bm(e) {
    return e === void 0 || typeof e == `string`;
  }
  function xm(e, t) {
    let n = new WeakMap(),
      r = [];
    outer: for (;;) {
      if (typeof e != `object` || !e) {
        if (!t(e)) return !1;
      } else {
        let t = e,
          i = n.get(t);
        if (i === !1) return !1;
        if (i === void 0) {
          let e = Array.isArray(t);
          if (!e) {
            let e = Object.getPrototypeOf(t);
            if (e !== null && e !== Object.prototype && Object.getPrototypeOf(e) !== null)
              return !1;
          }
          (n.set(t, !1), r.push({ value: t, keys: e ? t.length : Object.keys(t), index: 0 }));
        }
      }
      for (; r.length > 0;) {
        let t = r[r.length - 1],
          i = t.keys;
        if (typeof i == `number`) {
          if (t.index < i) {
            e = t.value[t.index++];
            continue outer;
          }
        } else if (t.index < i.length) {
          e = t.value[i[t.index++]];
          continue outer;
        }
        (n.set(t.value, !0), r.pop());
      }
      return !0;
    }
  }
  function Sm(e) {
    return xm(e, ym);
  }
  var Cm = new of([], () => (e, t, n) => (Sm(e) ? yu : Bc(new Mu(t, e, n))), {
      representation: { id: `effect/schema/Json`, payload: null },
      expected: `JSON value`,
      toCodecJson: () => void 0,
      toCodecStringTree: () => Dm,
    }),
    wm = new ef(Cm, jd()),
    Tm = new ef(new rp([new Ef(!1, [], [Cm]), new zf([], [new Rf(bf, Cm)])]), jd());
  function Em(e) {
    return xm(e, bm);
  }
  var Dm = new ef(
      new of([], () => (e, t, n) => (Em(e) ? yu : Bc(new Mu(t, e, n))), {
        expected: `StringTree`,
        toCodecStringTree: () => void 0,
      }),
      jd(),
    ),
    Om = (e, t, n) => (e === yu ? n(t) : ol(e, n));
  function km(e) {
    if (e._tag === `Middleware`)
      return (t, n, r) => Am(t === yu ? e.decode(_u(bu(n)), r) : e.decode(rl(t, bu), r));
    let t = e.decode;
    switch (t._tag) {
      case `Passthrough`:
        return (e, t) => (e === yu ? _u(t) : e);
      case `Transform`: {
        let e = (e) => (e === gu ? vu : _u(t.transform(e)));
        return (t, n) => Om(t, n, e);
      }
      case `TransformOptional`: {
        let e = (e) => xu(t.transform(bu(e)));
        return (t, n) => Om(t, n, e);
      }
      case `TransformEffect`:
        return (e, n, r) => Om(e, n, (e) => (e === gu ? vu : t.transform(e, r)));
      case `TransformOptionalEffect`:
        return (e, n, r) => Om(e, n, (e) => Am(t.transform(bu(e), r)));
    }
  }
  var Am = (e) => ol(e, xu),
    jm = (e, t, n, r) => Xc(r, (r) => Hc(() => pc(r, (r) => new Du(e, r, t, n))));
  function Mm(e, t) {
    let n = km(e.link.transformation),
      r;
    return (i, a) => {
      if (i === gu) return vu;
      if (e.isConstructed(i)) return yu;
      let o = (r ??= t(e.link.to))(i, a);
      return n(o, i, a);
    };
  }
  function Nm(e, t) {
    let n = e.context.constructorDefault;
    return (r, i) => {
      if (r !== gu && r !== void 0) return t(r, i);
      let a = n;
      if (so(a) && a._tag === `Success`) {
        let e = t(a[p], i);
        return e === yu ? a : e;
      }
      return ol(jm(e, r, i, a), (e) => {
        let n = t(e, i);
        return n === yu ? _u(e) : n;
      });
    };
  }
  function Pm(e, t) {
    let n = t(e);
    return e.context?.constructorDefault === void 0 ? n : Nm(e, n);
  }
  function Fm(e, t, n, r, i) {
    if (e._tag === `Declaration`) for (let n of e.typeParameters) t(n);
    let a = n ? _m(e) : void 0,
      o = a ? Mm(a, t) : (r ?? e.getParser(t, n)),
      s = e.checks,
      c = e.encoding,
      l = c?.map((e) => km(e.transformation)),
      u = e.encodingChecks;
    if (!c && !s && !u) return o;
    let d,
      f = (t, n) => {
        let r = o(t, n);
        if (u && !n.disableChecks) {
          if (so(r)) {
            if (r._tag === `Success`) {
              let i = r === yu ? t : r[p];
              if (t !== gu && i !== gu) {
                let i = gm(u, t, void 0, e, n);
                i && (r = Bc(new ju(e, i, t, n)));
              }
            }
          } else
            r = qc(r, (r) => {
              if (t !== gu && r !== gu) {
                let r = gm(u, t, void 0, e, n);
                if (r) return Bc(new ju(e, r, t, n));
              }
              return m(r);
            });
        }
        if (s && !n.disableChecks) {
          if (so(r)) {
            if (r._tag === `Success`) {
              let i = r === yu ? t : r[p];
              if (i === gu) return r;
              let a = gm(s, i, void 0, e, n);
              a && (r = Bc(new ju(e, a, i, n)));
            }
          } else
            r = qc(r, (t) => {
              if (t !== gu) {
                let r = gm(s, t, void 0, e, n);
                if (r) return Bc(new ju(e, r, t, n));
              }
              return m(t);
            });
        }
        return r;
      },
      ee = i === void 0 ? f : i(f);
    return c
      ? (n, r) => {
          let i = (d ??= c.map((e) => t(e.to))),
            a = n,
            o = i[i.length - 1](n, r);
          for (let e = c.length - 1; e >= 0; e--)
            if (((o = l[e](o, a, r)), e !== 0)) {
              let t = i[e - 1];
              o._tag === `Success`
                ? ((a = o[p]), (o = t(a, r)))
                : (o = ol(o, (e) => {
                    let n = t(e, r);
                    return n === yu ? _u(e) : n;
                  }));
            }
          if (o._tag === `Success`) {
            let e = o[p],
              t = ee(e, r);
            return t === yu ? o : t;
          }
          return (
            (o = jm(e, n, r, o)),
            ol(o, (e) => {
              let t = ee(e, r);
              return t === yu ? _u(e) : t;
            })
          );
        }
      : ee;
  }
  var Im = Symbol(),
    Lm = new WeakMap(),
    Rm,
    zm = (e) => Km(e).parser,
    Bm = (e) => Km(e).makeEffect,
    Vm = (e) => Pm(e, Bm),
    Hm = class {
      ast;
      constructor(e) {
        this.ast = e;
      }
      get decodeEffect() {
        return (this.cachedDecodeEffect ??= Fm(this.ast, zm));
      }
      get parser() {
        return this.decodeEffect;
      }
      get makeEffect() {
        return (this.cachedMakeEffect ??= Fm(this.ast, Bm, Vm));
      }
    },
    Um = class extends Hm {
      compiled;
      resolve;
      constructor(e, t, n) {
        (super(e), (this.compiled = t), (this.resolve = n));
      }
      save(e, t) {
        return (Object.defineProperty(this, e, { value: t }), t);
      }
      operation(e) {
        let t = this.compiled;
        return typeof t == `function` ? t(this.ast, this.resolve, e) : t?.[e];
      }
      get is() {
        return this.save(`is`, this.operation(`is`));
      }
      get decode() {
        return this.save(`decode`, this.operation(`decode`));
      }
      get make() {
        return this.save(`make`, this.operation(`make`));
      }
      get decodeEffect() {
        return this.save(
          `decodeEffect`,
          this.operation(`decodeEffect`) ?? Fm(this.ast, (e) => Gm(this.resolve, e, `parser`)),
        );
      }
      get parser() {
        let e = this.decode;
        return e === void 0
          ? this.decodeEffect
          : this.save(
              `parser`,
              Wm(e, () => this.decodeEffect),
            );
      }
      get makeEffect() {
        let e = this.operation(`makeEffect`);
        if (e !== void 0) return this.save(`makeEffect`, e);
        let t = (e) => Gm(this.resolve, e, `makeEffect`);
        return this.save(
          `makeEffect`,
          Fm(this.ast, t, (e) => Pm(e, t)),
        );
      }
    };
  function Wm(e, t) {
    let n;
    return (r, i) => {
      if (r !== gu)
        try {
          let t = e(r, i);
          if (t !== Im) return t === r ? yu : _u(t);
        } catch (e) {
          return Uc(e);
        }
      return (n ??= t())(r, i);
    };
  }
  function Gm(e, t, n) {
    let r = e(t);
    if (r.compiled === void 0 || Object.hasOwn(r, n)) return r[n];
    let i;
    return (e, t) => (i ??= r[n])(e, t);
  }
  function Km(e) {
    let t = Lm.get(e);
    if (t !== void 0) return t;
    let n = Rm === void 0 ? new Hm(e) : new Um(e, Rm(e, Km), Km);
    return (Lm.set(e, n), n);
  }
  function qm(e) {
    let t = e.ast,
      n;
    return (e, r) =>
      (n ??= oh(fh, Xp(t)))(
        e,
        r?.disableChecks
          ? r?.parseOptions
            ? { ...r.parseOptions, disableChecks: !0 }
            : { disableChecks: !0 }
          : r?.parseOptions,
      );
  }
  function Jm(e) {
    let t = qm(e);
    return (e, n) => {
      let r = nl(t(e, n));
      return Ms(r)
        ? tr(r.value)
        : (Qu(r.cause, `Option adapter can only return none for schema issues`), er());
    };
  }
  function Ym(e) {
    return uh(Xp(e.ast));
  }
  function Xm(e) {
    return Qm(e.ast);
  }
  function Zm(e) {
    {
      let t = sh(ih(e));
      return (e) => {
        let n = t(e, tf);
        return Ms(n)
          ? !0
          : (Qu(n.cause, `Type guard adapter can only return false for schema issues`), !1);
      };
    }
  }
  function Qm(e) {
    let t = Xp(e),
      n = (e) => ((n = Zm(t)), n(e));
    return (e) => n(e);
  }
  function $m(e, t) {
    let n = ih(e.ast);
    return t === void 0 ? n : (e, r) => n(e, nh(t, r));
  }
  function eh(e, t) {
    return ch($m(e, t));
  }
  function th(e, t) {
    let n = ih($p(e.ast));
    return t === void 0 ? n : (e, r) => n(e, nh(t, r));
  }
  var nh = (e, t) => (t ? { ...e, ...t } : e),
    rh = (e) => (e === gu ? Bc(new Nu()) : m(e));
  function ih(e) {
    return oh(dh, e);
  }
  function ah(e, t) {
    return e === yu ? m(t) : so(e) ? (e[p] === gu ? rh(gu) : e) : ol(e, rh);
  }
  function oh(e, t) {
    let n;
    return (r, i) => {
      let a = (n ??= e(t))(r, i ?? tf);
      return a === yu ? m(r) : so(a) ? (a[p] === gu ? rh(gu) : a) : ol(a, rh);
    };
  }
  function sh(e) {
    return (t, n) => nl(e(t, n));
  }
  function ch(e) {
    let t = sh(e);
    return (e, n) => {
      let r = t(e, n);
      return Ms(r)
        ? tr(r.value)
        : (Qu(r.cause, `Option adapter can only return none for schema issues`), er());
    };
  }
  function lh(e, t) {
    let n = nl(e);
    if (Ms(n)) return n.value;
    let r = Qu(n.cause, t);
    throw Error(`Schema validation failed`, { cause: r });
  }
  function uh(e) {
    let t, n;
    return (r, i) => {
      t ??= Km(e);
      let a = i?.disableChecks
          ? i.parseOptions
            ? { ...i.parseOptions, disableChecks: !0 }
            : { disableChecks: !0 }
          : (i?.parseOptions ?? tf),
        o = t.make;
      if (o !== void 0 && r !== gu) {
        let e;
        try {
          e = o(r, a);
        } catch (e) {
          throw (Qu(fc(e), `Constructor adapter can only throw schema issues`), e);
        }
        if (e !== Im && e !== gu) return e;
      }
      return lh(
        ah((n ??= t.makeEffect)(r, a), r),
        `Constructor adapter can only throw schema issues`,
      );
    };
  }
  var dh = (e) => Km(e).parser,
    fh = (e) => Km(e).makeEffect,
    ph = `~effect/Schema/Schema`,
    mh = Symbol(),
    hh = {
      [ph]: ph,
      get make() {
        let e = Ym(this);
        return (Object.defineProperty(this, "make", { value: e, enumerable: !0 }), e);
      },
      get makeEffect() {
        let e = qm(this);
        return (Object.defineProperty(this, "makeEffect", { value: e, enumerable: !0 }), e);
      },
      get makeOption() {
        let e = Jm(this);
        return (Object.defineProperty(this, "makeOption", { value: e, enumerable: !0 }), e);
      },
      pipe() {
        return n(this, arguments);
      },
      annotate(e) {
        return this.rebuild(Ep(this.ast, e));
      },
      annotateKey(e) {
        return this.rebuild(Rp(this.ast, e));
      },
      check(...e) {
        return this.rebuild(Op(this.ast, e));
      },
      rebuild(e) {
        return gh(e, this[mh]);
      },
    };
  function gh(e, t) {
    function n() {}
    let r = Object.setPrototypeOf(n, hh);
    return (
      t && (Object.hasOwn(t, `name`) || Object.hasOwn(t, `length`) || Object.hasOwn(t, `__proto__`))
        ? Object.defineProperties(r, Object.getOwnPropertyDescriptors({ ...t }))
        : Object.assign(r, t),
      (r[mh] = t),
      (r.ast = e),
      r
    );
  }
  function _h(e) {
    return gh(vh(e.ast), { schema: e });
  }
  var vh = Np((e) => {
    let t = Ch(e, vh),
      n = e.context;
    return t === e || n === void 0 ? t : Mp(t, yh(n));
  });
  function yh(e) {
    return e.constructorDefault === void 0
      ? e
      : new nf(e.isOptional, e.isMutable, void 0, e.annotations);
  }
  function bh(e) {
    if (e.propertySignatures.some((e) => typeof e.name != `string`))
      throw new globalThis.Error(`Objects property names must be strings`, { cause: e });
  }
  function xh(e) {
    return (t) => {
      let n = [...t].sort((t, n) => e(Zp(t)) - e(Zp(n)));
      return n.some((e, n) => e !== t[n]) ? n : t;
    };
  }
  var Sh = xh((e) => {
    switch (e._tag) {
      case `BigInt`:
      case `Symbol`:
      case `UniqueSymbol`:
        return 0;
      default:
        return 1;
    }
  });
  function Ch(e, t) {
    switch (e._tag) {
      case `Declaration`: {
        let n = e.annotations?.toCodecJson ?? e.annotations?.toCodec;
        if (!fe(n)) return Cp(e, [wm]);
        let r = n(e.typeParameters.map((e) => gh(Zp(e))));
        return r === void 0 ? e : Cp(e, [kp(r, t)]);
      }
      case `Unknown`:
        return Cp(e, [wm]);
      case `ObjectKeyword`:
        return Cp(e, [Tm]);
      case `Undefined`:
      case `Void`:
      case `Literal`:
      case `Number`:
        return e.toCodecJson();
      case `UniqueSymbol`:
      case `Symbol`:
      case `BigInt`:
        return e.toCodecStringTree();
      case `Objects`:
        return (bh(e), e.recur(t, am));
      case `Union`: {
        let n = Sh(e.types);
        return n === e.types
          ? e.recur(t)
          : new rp(
              n,
              e.options,
              e.annotations,
              e.checks,
              e.encoding,
              e.context,
              e.encodingChecks,
            ).recur(t);
      }
      case `Arrays`:
      case `Suspend`:
        return e.recur(t);
    }
    return e;
  }
  function wh(e) {
    return gh(Ah(e.ast), { schema: e });
  }
  var Th = xh((e) => {
    switch (e._tag) {
      case `Null`:
      case `Boolean`:
      case `Number`:
      case `BigInt`:
      case `Symbol`:
      case `UniqueSymbol`:
        return 0;
      default:
        return 1;
    }
  });
  function Eh(e, t, n) {
    switch (e._tag) {
      case `Declaration`: {
        let r = e.typeParameters.map((e) => gh(t(Zp(e)))),
          i = e.annotations?.toCodecStringTree;
        if (fe(i)) {
          let n = i(r);
          return n === void 0 ? e : Cp(e, [kp(n, t)]);
        }
        let a = e.annotations?.toCodecJson,
          o = fe(a) ? a(r) : void 0,
          s = o === void 0 ? e.annotations?.toCodec : void 0,
          c = o ?? (fe(s) ? s(r) : void 0);
        return c === void 0 ? n(e) : Cp(e, [kp(c, t)]);
      }
      case `Null`:
        return Cp(e, [Dh]);
      case `Boolean`:
        return Cp(e, [Oh]);
      case `Unknown`:
      case `ObjectKeyword`:
        return Cp(e, [Dm]);
      case `Enum`:
      case `Number`:
      case `Literal`:
      case `UniqueSymbol`:
      case `Symbol`:
      case `BigInt`:
        return e.toCodecStringTree();
      case `Objects`:
        return (bh(e), e.recur(t, am));
      case `Union`: {
        let n = Th(e.types);
        return n === e.types
          ? e.recur(t)
          : new rp(
              n,
              e.options,
              e.annotations,
              e.checks,
              e.encoding,
              e.context,
              e.encodingChecks,
            ).recur(t);
      }
      case `Arrays`:
      case `Suspend`:
        return e.recur(t);
    }
    return e;
  }
  var Dh = new ef(
      new vf(`null`),
      new Sd(
        ad(() => null),
        ad(() => `null`),
      ),
    ),
    Oh = new ef(
      new rp([new vf(`true`), new vf(`false`)]),
      new Sd(
        ad((e) => e === `true`),
        dd(),
      ),
    ),
    kh = new Sd(
      ad((e) => (typeof e == `string` ? [e] : e)),
      id(),
    ),
    Ah = Np(
      (e) => {
        let t = Eh(e, Ah, (e) => {
          throw new globalThis.Error(`Missing structural codec for StringTree`, { cause: e });
        });
        return t !== e && e.context !== void 0 ? Mp(t, yh(e.context)) : t;
      },
      { stopAt: (e) => e.transformation === kh },
    ),
    jh = a(2, (e, t) => ({ ...e, ...t })),
    Mh = (e) => e,
    Nh = `~effect/Schema/SchemaError`;
  function Ph(e) {
    return (
      ye(e, `~effect/Schema/SchemaError`) &&
      e[`~effect/Schema/SchemaError`] === `~effect/Schema/SchemaError`
    );
  }
  var Fh = ph;
  function Ih() {
    return (e, t, n) => Zh(new of(e.map(Kf), (e) => t(e.map((e) => Zh(e))), n));
  }
  function Lh(e, t) {
    return Ih()([], () => (t, n, r) => (e(t) ? m(t) : Bc(new Mu(n, t, r))), t);
  }
  function Rh(e) {
    return (t) => t.annotate(e);
  }
  function zh(e) {
    return (t) => t.rebuild(Rp(t.ast, e));
  }
  var Bh = class extends $i(`SchemaError`) {
    [Nh] = Nh;
    constructor(e) {
      let t = Ct();
      wt(0);
      try {
        super({ issue: e });
      } finally {
        wt(t);
      }
    }
    get message() {
      return Ju(this.issue);
    }
    toString() {
      return `SchemaError(${this.message})`;
    }
  };
  function Vh(e) {
    return Ph(e);
  }
  function Hh(e) {
    return so(e) ? Uh(e) : Xc(e, (e) => Hc(() => pc(e, (e) => new Bh(e))));
  }
  function Uh(e) {
    return Ms(e) ? e : ks(pc(e.cause, (e) => new Bh(e)));
  }
  function Wh(e, t) {
    let n;
    for (let r of e.reasons) {
      if (!dc(r) || !Vh(r.error)) throw new globalThis.Error(t, { cause: e });
      n ??= r.error;
    }
    if (n === void 0) throw new globalThis.Error(t, { cause: e });
    return n;
  }
  function Gh(e) {
    let t = nl(e);
    if (Ms(t)) return t.value;
    throw Wh(t.cause, `Sync adapter can only throw schema errors`);
  }
  var Kh = Xm;
  function qh(e, t) {
    let n = $m(e, t);
    return (e, t) => Hh(n(e, t));
  }
  var Jh = eh;
  function Yh(e, t) {
    let n = qh(e, t);
    return (e, t) => Gh(n(e, t));
  }
  var Xh = Yh,
    Zh = gh;
  function Qh(e) {
    return ye(e, Fh) && e[Fh] === Fh;
  }
  var h = Mh((e) => Zh(zp(e.ast), { schema: e })),
    g = Mh((e) => {
      let t = dg(e);
      return Zh(Vp(e.ast), { schema: t });
    }),
    $h = Mh((e) => Zh(Xp(e.ast), { schema: e })),
    eg = Mh((e) => Zh(Zp(e.ast), { schema: e }));
  function _(e) {
    let t = Zh(new vf(e), {
      literal: e,
      transform(n) {
        return t.pipe(xg(_(n), { decode: ad(() => n), encode: ad(() => e) }));
      },
    });
    return t;
  }
  function tg(e) {
    return e.map((e) => (Qh(e) ? e.ast : new vf(e)));
  }
  function ng(e) {
    return Zh(new gf(tg(e)), { parts: e });
  }
  var rg = Zh(ff),
    v = Zh(pf),
    ig = Zh(sf),
    ag = Zh(uf),
    y = Zh(bf),
    b = Zh(Cf),
    x = Zh(wf),
    og = Zh(Tf),
    sg = Zh(df);
  function cg(e, t) {
    return Zh(e, {
      fields: t,
      mapFields(e, t) {
        let n = e(this.fields);
        return cg(Gf(n, t?.unsafePreserveChecks ? this.ast.checks : void 0), n);
      },
    });
  }
  function S(e) {
    return cg(Gf(e, void 0), e);
  }
  function C(e, t) {
    return Zh(Gp(e.ast, t.ast), { key: e, value: t });
  }
  var w = Mh((e) => Zh(new Ef(!1, [], [e.ast]), { value: e })),
    lg = Mh((e) => Zh(Jf(e.ast), { schema: e }));
  function ug(e, t) {
    return Zh(e, {
      members: t,
      mapMembers(e, t) {
        let n = e(this.members);
        return ug(qf(n, this.ast.options, t?.unsafePreserveChecks ? this.ast.checks : void 0), n);
      },
    });
  }
  function T(e, t) {
    return ug(qf(e, t, void 0), e);
  }
  function E(e) {
    let t = e.map(_);
    return Zh(qf(t, void 0, void 0), {
      literals: e,
      members: t,
      mapMembers(e) {
        return T(e(this.members));
      },
      pick(e) {
        return E(e);
      },
      transform(e) {
        return T(t.map((t, n) => t.transform(e[n])));
      },
    });
  }
  var D = Mh((e) => T([e, ig])),
    dg = Mh((e) => T([e, ag]));
  function fg(e) {
    return Zh(new fp(() => e().ast));
  }
  function pg(...e) {
    return (t) => t.check(...e);
  }
  function mg(e) {
    return (t) => Zh(t.ast, { schema: t, identifier: e });
  }
  function hg(e) {
    return (t) => Zh(Pp(t.ast, new bd(e, o)), { schema: t });
  }
  function gg(e) {
    return (t) => Zh(Fp(t.ast, new bd(o, e)), { schema: t });
  }
  function _g(e) {
    return vg(e);
  }
  function vg(e) {
    return (t) => hg(sl(e))(t);
  }
  function yg(e) {
    return bg(e);
  }
  function bg(e) {
    return (t) => gg(sl(e))(t);
  }
  function xg(e, t) {
    return (n) => Zh(Up(n.ast, e.ast, t ? wd(t) : jd()), { from: n, to: e });
  }
  function Sg() {
    return (e, t) => kg()(e, { decode: t, encode: nd });
  }
  function Cg(e) {
    return (t) => Zh(Hp(t.ast, e), { schema: t });
  }
  function wg(e) {
    return Xc(e, (e) => Hc(() => pc(e, (e) => e.issue)));
  }
  function Tg(e, t) {
    let n = t?.encodingStrategy === `omit` ? ld() : id();
    return (t) => h(eg(t)).pipe(xg(t, { decode: ud(wg(e)), encode: n }));
  }
  function O(e, t) {
    let n = t?.encodingStrategy === `omit` ? ld() : id();
    return (t) => g(eg(t)).pipe(xg(t, { decode: ud(wg(e)), encode: n }));
  }
  function Eg(e) {
    return _(e).pipe(Cg(m(e)));
  }
  function Dg(e, t) {
    return S({ _tag: Eg(e), ...t });
  }
  function Og(e, t) {
    return Lh((t) => t instanceof e, t);
  }
  function kg() {
    return (e, t) => new ef(e.ast, wd(t));
  }
  var k = hp,
    Ag = `^\\S[\\s\\S]*\\S$|^\\S$|^$`;
  function jg(e) {
    let t = new globalThis.RegExp(Ag);
    return k((e) => e.trim() === e, {
      expected: `a string with no leading or trailing whitespace`,
      representation: { id: `effect/schema/isTrimmed`, payload: null },
      toJsonSchema: () => ({ pattern: t.source }),
      toCode: () => ({ runtime: `Schema.isTrimmed()` }),
      arbitraryConstraint: { patterns: [{ source: Ag, flags: `` }] },
      ...e,
    });
  }
  function Mg(e, t) {
    let n = e.source,
      r = e.flags,
      i = /^[dg]*uy?$/.test(r),
      a = r === `` ? `new RegExp(${lt(n)})` : `new RegExp(${lt(n)}, ${lt(r)})`;
    return yp(e, {
      toJsonSchema: () => (i ? { pattern: r.endsWith(`y`) ? `^(?:${n})` : n } : [{}, !0]),
      toCode: () => ({ runtime: `Schema.isPattern(${a})` }),
      ...t,
    });
  }
  var Ng = Zh(_p),
    Pg = gp;
  function Fg(e) {
    let t = Zn(e.order),
      n = e.formatter ?? lt;
    return (r, i) =>
      k((e) => t(e, r), {
        expected: `a value greater than ${n(r)}`,
        arbitraryConstraint: { order: e.order, minimum: r, exclusiveMinimum: !0 },
        ...e.annotate?.(r),
        ...i,
      });
  }
  function Ig(e) {
    let t = $n(e.order),
      n = e.formatter ?? lt;
    return (r, i) =>
      k((e) => t(e, r), {
        expected: `a value greater than or equal to ${n(r)}`,
        arbitraryConstraint: { order: e.order, minimum: r },
        ...e.annotate?.(r),
        ...i,
      });
  }
  function Lg(e) {
    let t = Xn(e.order),
      n = e.formatter ?? lt;
    return (r, i) =>
      k((e) => t(e, r), {
        expected: `a value less than ${n(r)}`,
        arbitraryConstraint: { order: e.order, maximum: r, exclusiveMaximum: !0 },
        ...e.annotate?.(r),
        ...i,
      });
  }
  function Rg(e) {
    let t = Qn(e.order),
      n = e.formatter ?? lt;
    return (r, i) =>
      k((e) => t(e, r), {
        expected: `a value less than or equal to ${n(r)}`,
        arbitraryConstraint: { order: e.order, maximum: r },
        ...e.annotate?.(r),
        ...i,
      });
  }
  function zg(e) {
    let t = $n(e.order),
      n = Zn(e.order),
      r = Qn(e.order),
      i = Xn(e.order),
      a = e.formatter ?? lt;
    return (o, s) => {
      let c = o.exclusiveMinimum ? n : t,
        l = o.exclusiveMaximum ? i : r;
      return k((e) => c(e, o.minimum) && l(e, o.maximum), {
        expected: `a value between ${a(o.minimum)}${o.exclusiveMinimum ? ` (excluded)` : ``} and ${a(o.maximum)}${o.exclusiveMaximum ? ` (excluded)` : ``}`,
        arbitraryConstraint: {
          order: e.order,
          minimum: o.minimum,
          maximum: o.maximum,
          ...(o.exclusiveMinimum && { exclusiveMinimum: !0 }),
          ...(o.exclusiveMaximum && { exclusiveMaximum: !0 }),
        },
        ...e.annotate?.(o),
        ...s,
      });
    };
  }
  function Bg(e) {
    if (!globalThis.Number.isFinite(e))
      throw new globalThis.RangeError(`Expected a finite number, got ${lt(e)}`);
    return e;
  }
  var Vg = Fg({
      order: Yn,
      annotate: (e) => ({
        representation: { id: `effect/schema/isGreaterThan`, payload: { exclusiveMinimum: Bg(e) } },
        toJsonSchema: () => ({ exclusiveMinimum: e }),
        toCode: () => ({ runtime: `Schema.isGreaterThan(${lt(e)})` }),
      }),
    }),
    Hg = Ig({
      order: Yn,
      annotate: (e) => ({
        representation: { id: `effect/schema/isGreaterThanOrEqualTo`, payload: { minimum: Bg(e) } },
        toJsonSchema: () => ({ minimum: e }),
        toCode: () => ({ runtime: `Schema.isGreaterThanOrEqualTo(${lt(e)})` }),
      }),
    }),
    Ug = Lg({
      order: Yn,
      annotate: (e) => ({
        representation: { id: `effect/schema/isLessThan`, payload: { exclusiveMaximum: Bg(e) } },
        toJsonSchema: () => ({ exclusiveMaximum: e }),
        toCode: () => ({ runtime: `Schema.isLessThan(${lt(e)})` }),
      }),
    }),
    Wg = Rg({
      order: Yn,
      annotate: (e) => ({
        representation: { id: `effect/schema/isLessThanOrEqualTo`, payload: { maximum: Bg(e) } },
        toJsonSchema: () => ({ maximum: e }),
        toCode: () => ({ runtime: `Schema.isLessThanOrEqualTo(${lt(e)})` }),
      }),
    }),
    Gg = zg({
      order: Yn,
      annotate: (e) => {
        let t = e.exclusiveMinimum ? !0 : void 0,
          n = e.exclusiveMaximum ? !0 : void 0;
        return {
          representation: {
            id: `effect/schema/isBetween`,
            payload: {
              minimum: Bg(e.minimum),
              maximum: Bg(e.maximum),
              ...(t && { exclusiveMinimum: t }),
              ...(n && { exclusiveMaximum: n }),
            },
          },
          toJsonSchema: () => ({
            [t ? `exclusiveMinimum` : `minimum`]: e.minimum,
            [n ? `exclusiveMaximum` : `maximum`]: e.maximum,
          }),
          toCode: () => ({
            runtime: `Schema.isBetween({ minimum: ${lt(e.minimum)}, maximum: ${lt(e.maximum)}, exclusiveMinimum: ${lt(t)}, exclusiveMaximum: ${lt(n)} })`,
          }),
        };
      },
    });
  function Kg(e) {
    return k((e) => globalThis.Number.isSafeInteger(e), {
      expected: `an integer`,
      representation: { id: `effect/schema/isInt`, payload: null },
      toJsonSchema: () => [{ type: `integer` }, !0],
      toCode: () => ({ runtime: `Schema.isInt()` }),
      arbitraryConstraint: { number: `integer` },
      ...e,
    });
  }
  var A = b.check(Kg());
  function qg(e, t) {
    return ((e = Xg(e)), Jg(e, Math.ceil(e / 2), t));
  }
  function Jg(e, t, n) {
    return k((t) => t.length >= e, {
      expected: `a value with a length of at least ${e}`,
      representation: { id: `effect/schema/isMinLength`, payload: { minLength: e } },
      toJsonSchema: ({ type: n }) =>
        n === `string`
          ? e <= 1
            ? { minLength: t }
            : [{ minLength: t }, !0]
          : n === `array`
            ? { minItems: e }
            : n === void 0
              ? [{ minLength: t, minItems: e }, !0]
              : [{}, !0],
      toCode: () => ({ runtime: `Schema.isMinLength(${e})` }),
      [fu]: !0,
      arbitraryConstraint: { minLength: e },
      ...n,
    });
  }
  function Yg(e) {
    return Jg(1, 1, e);
  }
  function j(e, t) {
    return (
      (e = Xg(e)),
      k((t) => t.length <= e, {
        expected: `a value with a length of at most ${e}`,
        representation: { id: `effect/schema/isMaxLength`, payload: { maxLength: e } },
        toJsonSchema: ({ type: t }) =>
          t === `string`
            ? e === 0
              ? { maxLength: e }
              : [{ maxLength: e }, !0]
            : t === `array`
              ? { maxItems: e }
              : t === void 0
                ? [{ maxLength: e, maxItems: e }, !0]
                : [{}, !0],
        toCode: () => ({ runtime: `Schema.isMaxLength(${e})` }),
        [fu]: !0,
        arbitraryConstraint: { maxLength: e },
        ...t,
      })
    );
  }
  function Xg(e) {
    if (!globalThis.Number.isFinite(e))
      throw new globalThis.RangeError(`Expected a finite number, got ${e}`);
    return Math.max(0, Math.floor(e));
  }
  function Zg(e, t) {
    return (
      (e = Xg(e)),
      k((t) => Reflect.ownKeys(t).length <= e, {
        expected: `a value with at most ${e === 1 ? `1 entry` : `${e} entries`}`,
        representation: { id: `effect/schema/isMaxProperties`, payload: { maxProperties: e } },
        toJsonSchema: ({ type: t }) =>
          t === `object` ? { maxProperties: e } : [{ maxProperties: e }, !0],
        toCode: () => ({ runtime: `Schema.isMaxProperties(${e})` }),
        [fu]: !0,
        arbitraryConstraint: { maxProperties: e },
        ...t,
      })
    );
  }
  var Qg = y.check(Yg()),
    $g = (e) => +(e?.includeStack === !0) | (e?.excludeCause === !0 ? 2 : 0),
    e_ = (e) => {
      switch (e) {
        case 0:
          return;
        case 1:
          return { includeStack: !0 };
        case 2:
          return { excludeCause: !0 };
        case 3:
          return { includeStack: !0, excludeCause: !0 };
      }
    },
    t_ = [];
  function M(e) {
    let t = $g(e),
      n = t_[t];
    if (n !== void 0) return n;
    let r = D_.pipe(xg(v, Bd(e_(t))));
    return ((t_[t] = r), r);
  }
  (globalThis.RegExp, globalThis.URL);
  var n_ = y.annotate({
    expected: `a string that will be decoded as JSON`,
    contentMediaType: `application/json`,
  });
  function r_(e, t) {
    return n_.pipe(xg(e, Ud(t)));
  }
  (globalThis.File, globalThis.FormData, globalThis.URLSearchParams);
  var i_ = y.annotate({
      expected: `a base64 encoded string that will be decoded as Uint8Array`,
      format: `byte`,
      contentEncoding: `base64`,
    }),
    a_ = Og(globalThis.Uint8Array, {
      representation: { id: `effect/schema/Uint8Array`, payload: null },
      toCode: () => ({ runtime: `Schema.Uint8Array`, Type: `globalThis.Uint8Array` }),
      expected: `Uint8Array`,
      toCodecJson: () => kg()(i_, Hd),
    }),
    o_ = Td({
      decode: (e, t) =>
        ar(Il(e), {
          onNone: () => Bc(new Nu({ expected: `a valid UTC DateTime string` }, e, t)),
          onSome: (e) => m(Ll(e)),
        }),
      encode: (e) => m(Rl(e)),
    }),
    s_ = -864e13,
    c_ = 864e13;
  function l_(e, t, n) {
    let r = Math.max(
        t,
        e?.minimum === void 0 ? t : e.minimum.epochMilliseconds + +(e.exclusiveMinimum === !0),
      ),
      i = Math.min(
        n,
        e?.maximum === void 0 ? n : e.maximum.epochMilliseconds - +(e.exclusiveMaximum === !0),
      );
    return r <= i ? [r, i] : [t, n];
  }
  function u_(e, t) {
    return A.check(Gg({ minimum: e, maximum: t }));
  }
  var N = Lh((e) => Nl(e) && Pl(e), {
      representation: { id: `effect/schema/DateTimeUtc`, payload: null },
      toCode: () => ({
        runtime: `Schema.DateTimeUtc`,
        Type: `DateTime.Utc`,
        importDeclarations: [`import * as DateTime from "effect/DateTime"`],
      }),
      expected: `DateTime.Utc`,
      toCodecArbitrary: ({ constraint: e }) => {
        let [t, n] = l_(e, s_, c_);
        return Sg()(u_(t, n), ad(Fl));
      },
      toCodecJson: () => kg()(y, o_),
      toFormatter: () => (e) => e.toString(),
    }),
    P = y.annotate({ expected: `a string that will be decoded as a DateTime.Utc` }).pipe(xg(N, o_)),
    d_ = Ed({ decode: (e) => pi(e), encode: (e) => yi(e) }),
    f_ = Lh(ci, {
      representation: { id: `effect/schema/Duration`, payload: null },
      toCode: () => ({
        runtime: `Schema.Duration`,
        Type: `Duration.Duration`,
        importDeclarations: [`import * as Duration from "effect/Duration"`],
      }),
      expected: `Duration`,
      toCodecJson: () =>
        kg()(
          T([
            S({ _tag: _(`Infinity`) }),
            S({ _tag: _(`NegativeInfinity`) }),
            S({ _tag: _(`Nanos`), value: og }),
            S({ _tag: _(`Millis`), value: A }),
          ]),
          Ed({
            decode: (e) => {
              switch (e._tag) {
                case `Infinity`:
                  return ui;
                case `NegativeInfinity`:
                  return di;
                case `Nanos`:
                  return fi(e.value);
                case `Millis`:
                  return pi(e.value);
              }
            },
            encode: (e) => {
              switch (e.value._tag) {
                case `Infinity`:
                  return { _tag: `Infinity` };
                case `NegativeInfinity`:
                  return { _tag: `NegativeInfinity` };
                case `Nanos`:
                  return { _tag: `Nanos`, value: e.value.nanos };
                case `Millis`:
                  return { _tag: `Millis`, value: e.value.millis };
              }
            },
          }),
        ),
    }),
    p_ = b.pipe(xg(f_, d_));
  function m_(e) {
    return Zh(
      Ih()(
        [e],
        ([e]) =>
          (t, n, r) =>
            nr(t)
              ? rr(t)
                ? Ic
                : al($m(e)(t.value, r), {
                    onSuccess: tr,
                    onFailure: (e) => Pu(n, `value`, e, t, r),
                  })
              : Bc(new Mu(n, t, r)),
        {
          representation: { id: `effect/schema/Option`, payload: null },
          toCode: ({ typeParameters: e }) => ({
            runtime: `Schema.Option(${e[0].runtime})`,
            Type: `Option.Option<${e[0].Type}>`,
            importDeclarations: [`import * as Option from "effect/Option"`],
          }),
          expected: `Option`,
          toCodec: ([e]) =>
            kg()(
              T([S({ _tag: _(`Some`), value: e }), S({ _tag: _(`None`) })]),
              Ed({
                decode: (e) => (e._tag === `None` ? er() : tr(e.value)),
                encode: (e) => (ir(e) ? { _tag: `Some`, value: e.value } : { _tag: `None` }),
              }),
            ),
        },
      ).ast,
      { value: e },
    );
  }
  function h_(e) {
    return D(e).pipe(xg(m_($h(e)), Vd()));
  }
  var g_ = globalThis.Symbol.for(`immer-draftable`),
    __ = {};
  function v_(e, t, r, i, a) {
    let o = x_(r, t, i),
      s = b_(t),
      c = class extends e {
        constructor(...[e, t]) {
          let n = t?.[`~payload`],
            i = n?.token === __ ? n.value : r.make(e ?? {}, t);
          super(i, { ...t, disableChecks: !0, "~payload": { token: __, value: i } });
        }
        static [Fh] = Fh;
        get [s]() {
          return s;
        }
        static [g_] = !0;
        static identifier = t;
        static fields = r.fields;
        static get ast() {
          return o(this).ast;
        }
        static pipe() {
          return n(this, arguments);
        }
        static rebuild(e) {
          return o(this).rebuild(e);
        }
        static make(e, t) {
          return Ym(o(this))(e ?? {}, t);
        }
        static makeOption(e, t) {
          return Jm(o(this))(e ?? {}, t);
        }
        static makeEffect(e, t) {
          return o(this).makeEffect(e ?? {}, t);
        }
        static annotate(e) {
          return this.rebuild(Ep(this.ast, e));
        }
        static annotateKey(e) {
          return this.rebuild(Rp(this.ast, e));
        }
        static check(...e) {
          return this.rebuild(Op(this.ast, e));
        }
        static extend(e) {
          return (t, n) => {
            let i = S_(t) ? t : S(t),
              o = { ...r.fields, ...i.fields },
              s = Gf(o, r.ast.checks, { identifier: e });
            return v_(this, e, cg(Op(s, i.ast.checks), o), n, a);
          };
        }
        static mapFields(e, t) {
          return r.mapFields(e, t);
        }
      };
    return (a !== void 0 && Object.assign(c.prototype, a(t)), c);
  }
  function y_(e) {
    return new Sd(
      ad((t) => new e(t, { "~payload": { token: __, value: t } })),
      id(),
    );
  }
  function b_(e) {
    return `~effect/Schema/Class/${e}`;
  }
  function x_(e, t, n) {
    let r;
    return (i) => {
      if (r !== void 0) return r;
      let a = b_(t),
        o = (e) => e instanceof i || ye(e, a),
        s = y_(i);
      return (r = xg(
        Zh(
          new of([e.ast], () => (e, t, n) => (o(e) ? m(e) : Bc(new Mu(t, e, n))), {
            identifier: t,
            [mu]: ([e]) => ({ isConstructed: o, link: new ef(e, s) }),
            toCodec: ([e]) => new ef(e.ast, s),
            toEquivalence: ([e]) => e,
            toFormatter:
              ([e]) =>
              (t) =>
                `${i.identifier}(${e(t)})`,
            [pu]: Zf(e.ast),
            ...n,
          }),
        ),
        s,
      )(e));
    };
  }
  function S_(e) {
    return Qh(e);
  }
  var C_ = (e) => (t, n) =>
      v_(Qi, e, S_(t) ? t : S(t), n, (e) => ({
        toString() {
          return `${e}(${lt({ ...this })})`;
        },
      })),
    w_ = (e) => (t, n) => v_(yn, e, S_(t) ? t : S(t), n, (e) => ({ name: e })),
    F = (e) => (t, n, r) => {
      let i = S_(n)
        ? n.mapFields((e) => ({ _tag: Eg(t), ...e }), { unsafePreserveChecks: !0 })
        : Dg(t, n);
      return w_(e ?? t)(i, r);
    },
    T_ = _h,
    E_ = wh,
    D_ = Zh(Ep(Cm, { toCode: () => ({ runtime: `Schema.Json`, Type: `Schema.Json` }) })),
    I = y.pipe(xg(y, Td({ decode: (e) => m(e.trim()), encode: (e) => m(e.trim()) }))),
    O_ = k((e) => e.trim().length > 0, {
      expected: `a non-blank string`,
      toJsonSchema: () => [{ minLength: 1 }, !0],
      arbitraryConstraint: { minLength: 1 },
    }),
    L = I.check(O_),
    R = A.check(Hg(0)),
    z = A.check(Hg(1)),
    k_ = A.check(Gg({ minimum: 1, maximum: 65535 })),
    A_ = E([
      `time_window`,
      `key_mismatch`,
      `request_mismatch`,
      `token_mismatch`,
      `replay`,
      `invalid_proof`,
    ]),
    B = y,
    j_ = (e) => {
      let t = Jh(e);
      return h(v.pipe(xg(dg(e), Ed({ decode: (e) => (ir(t(e)) ? e : void 0), encode: (e) => e }))));
    },
    M_ = (e) => {
      let t = Jh(e);
      return v.pipe(xg(D(e), Ed({ decode: (e) => (ir(t(e)) ? e : null), encode: (e) => e })));
    },
    N_ = (e) => {
      let t = Jh(e);
      return h(v).pipe(
        xg(
          D(e),
          Dd({
            decode: (e) => tr(ir(e) && ir(t(e.value)) ? e.value : null),
            encode: (e) => (ir(e) && e.value !== null ? tr(e.value) : er()),
          }),
        ),
      );
    },
    P_ = (e) =>
      w(
        dg(e).pipe(
          _g(() => Lc(void 0)),
          yg(() => Lc(void 0)),
        ),
      ).pipe(
        xg(
          w(dg($h(e)).pipe(yg(() => Lc(void 0)))).check(
            k(
              (e) => {
                for (let t = 0; t < e.length; t++) if (e[t] === void 0) return !1;
                return !0;
              },
              { expected: `an array without holes` },
              !0,
            ),
          ),
          Ed({
            decode: (e) => e.filter((e) => e !== void 0),
            encode: (e) => e.filter((e) => e !== void 0),
          }),
        ),
      ),
    F_ = class {
      tag;
      value;
      constructor(e, t) {
        ((this.tag = e), (this.value = t));
      }
    },
    I_ = (e, t) => {
      let n = B_(e, t),
        r = S({ [t]: y.check(k((e) => !n.has(e) || `A known ${t} must decode in full.`)) }).pipe(
          xg(Og(F_), {
            decode: ad((e) => new F_(t, e[t])),
            encode: td(() => `Unknown ${t} values are never sent.`),
          }),
        );
      return T([...e, r]);
    },
    L_ = (e, t) => {
      let n = B_(e, t);
      return (e) => typeof e == `object` && !!e && t in e && !n.has(e[t]);
    },
    R_ = (e) => e instanceof F_,
    z_ = (e, t) =>
      w(I_(e, t)).pipe(
        xg(w($h(T(e))), Ed({ decode: (e) => e.filter((e) => !R_(e)), encode: (e) => e })),
      ),
    B_ = (e, t) => {
      let n = new Set();
      for (let r of e) {
        let e = r.fields[t];
        for (let r of e?.literals ?? [e?.literal]) {
          if (typeof r != `string`)
            throw Error(`Every ForwardCompatibleUnion member needs a string literal ${t}.`);
          n.add(r);
        }
      }
      return n;
    },
    V_ = (e) => L.pipe(mg(e)),
    V = V_(`ThreadId`),
    H = V_(`ProjectId`),
    U = V_(`EnvironmentId`),
    W = V_(`CommandId`),
    H_ = V_(`EventId`),
    U_ = V_(`MessageId`),
    W_ = V_(`TurnId`),
    G = V_(`RunId`),
    G_ = V_(`RunAttemptId`),
    K = V_(`NodeId`),
    K_ = V_(`AuthSessionId`),
    q_ = R.pipe(mg(`RpcClientId`)),
    J_ = E([`web`, `desktop`, `mobile`, `cli`]);
  (E([`macOS`, `Windows`, `Linux`, `iOS`, `Android`, `ChromeOS`, `other`, `unknown`]),
    E([`desktop`, `phone`, `tablet`, `unknown`]));
  var Y_ = E([`hosted`, `server`]);
  E([`direct`, `ssh`, `relay`, `unknown`]);
  var X_ = V_(`ProviderItemId`),
    Z_ = V_(`ProviderSessionId`),
    Q_ = V_(`ProviderThreadId`),
    $_ = V_(`ProviderTurnId`);
  V_(`RuntimeSessionId`);
  var ev = V_(`RuntimeItemId`),
    tv = V_(`TurnItemId`),
    nv = V_(`RuntimeRequestId`),
    rv = V_(`RuntimeTaskId`),
    iv = V_(`ScheduledTaskId`),
    av = V_(`SecretRef`),
    ov = V_(`ApprovalRequestId`),
    sv = V_(`CheckpointRef`),
    cv = V_(`CheckpointId`),
    lv = V_(`CheckpointScopeId`),
    uv = V_(`ContextHandoffId`),
    dv = V_(`ContextTransferId`),
    fv = V_(`RawEventId`),
    pv = V_(`PlanId`);
  S({
    version: _(1),
    environmentId: U.check(j(512)),
    threadId: V.check(j(512)),
    messageId: U_.check(j(512)),
    text: y.check(Yg(), j(8e3)),
    comment: g(y.check(j(8e3))),
    start: R.check(Wg(2 ** 53 - 1)),
    end: R.check(Wg(2 ** 53 - 1)),
    prefix: y.check(j(32)),
    suffix: y.check(j(32)),
  }).check(k((e) => e.end > e.start && e.text.trim().length > 0));
  var mv = [
      `image`,
      `file`,
      `terminal`,
      `element`,
      `preview-annotation`,
      `review-comment`,
      `mention`,
      `skill`,
      `thread`,
    ],
    hv = 128,
    gv = /^[a-z0-9_-]+$/i,
    _v = /^[a-z][a-z0-9-]{0,39}$/,
    vv = RegExp(`^(?!(?:${mv.join(`|`)})$)`),
    yv = 64e3,
    bv = 8e3,
    xv = 8e3,
    Sv = 16e3,
    Cv = 32e3,
    wv = 8e3,
    Tv = L.check(j(hv), Mg(gv)).pipe(mg(`ComposerContextId`));
  L.check(j(hv), Mg(gv)).pipe(mg(`ComposerContextReferenceId`));
  var Ev = L.check(Mg(_v)),
    Dv = L.check(j(hv), Mg(gv)),
    Ov = y.check(j(200)),
    kv = (e) => y.check(j(e)),
    Av = kv(2048),
    jv = D(Av),
    Mv = S({
      number: z,
      title: Av,
      url: Av,
      headBranch: Av,
      baseBranch: Av,
      state: E([`open`, `closed`, `merged`]),
      isDraft: x,
    }),
    Nv = { version: _(1), contextId: Tv, label: Ov },
    Pv = { attachmentId: Dv, name: L.check(j(255)), mimeType: L.check(j(100)), sizeBytes: R },
    Fv = S({ ...Nv, kind: _(`image`), ...Pv }),
    Iv = S({ ...Nv, kind: _(`file`), ...Pv }),
    Lv = S({
      ...Nv,
      kind: _(`terminal`),
      terminalId: L.check(j(255)),
      terminalLabel: L.check(j(255)),
      lineStart: R,
      lineEnd: R,
      text: kv(yv),
    }).check(k((e) => e.lineEnd >= e.lineStart)),
    Rv = S({ functionName: jv, fileName: jv, lineNumber: D(R), columnNumber: D(R) }),
    zv = S({
      pageUrl: Av,
      pageTitle: jv,
      tagName: L.check(j(255)),
      selector: jv,
      htmlPreview: kv(bv),
      componentName: jv,
      source: D(Rv),
      styles: kv(xv),
    }),
    Bv = S({ ...Nv, kind: _(`element`), ...zv.fields }),
    Vv = S({
      ...Nv,
      kind: _(`preview-annotation`),
      annotationId: Av,
      pageUrl: Av,
      pageTitle: jv,
      comment: kv(wv),
      targetSummary: Av,
      styleChanges: w(Av).check(j(200)),
      elements: h(w(zv).check(j(50))),
      elementIds: h(w(Av).check(j(50))),
      regionCount: h(R),
      strokeCount: h(R),
      styleChangeDetails: h(
        w(
          S({ targetId: Av, selector: jv, property: Av, previousValue: kv(xv), value: kv(xv) }),
        ).check(j(200)),
      ),
      screenshotContextId: h(Tv),
    }),
    Hv = S({
      ...Nv,
      kind: _(`review-comment`),
      sectionId: L.check(j(255)),
      sectionTitle: Av,
      filePath: L.check(j(2048)),
      startIndex: R,
      endIndex: R,
      rangeLabel: Av,
      text: kv(Sv),
      diff: kv(Cv),
      fenceLanguage: h(kv(64)),
      pullRequest: h(Mv),
    }).check(k((e) => e.endIndex >= e.startIndex)),
    Uv = S({ ...Nv, kind: _(`mention`), path: L.check(j(2048)) }),
    Wv = S({ ...Nv, kind: _(`skill`), name: L.check(j(255)) }),
    Gv = S({ ...Nv, kind: _(`thread`), environmentId: U, threadId: V, title: Ov }),
    Kv = Kh(D_),
    qv = S({
      ...Nv,
      kind: Ev.check(Mg(vv)),
      payload: v.check(
        k((e) => {
          let t;
          try {
            t = JSON.stringify(e);
          } catch {
            return !1;
          }
          return t !== void 0 && t.length <= 64e3 && Kv(e);
        }),
      ),
    }),
    Jv = T([...T([Fv, Iv, Lv, Bv, Vv, Hv, Uv, Wv, Gv]).members, qv]),
    Yv = 16e6,
    Xv = S({
      version: _(1),
      records: w(v)
        .check(
          j(200),
          k((e) => {
            try {
              return JSON.stringify(e).length <= Yv;
            } catch {
              return !1;
            }
          }),
        )
        .pipe(xg(P_(Jv)))
        .check(k((e) => new Set(e.map((e) => e.contextId)).size === e.length)),
    });
  S({
    version: _(1),
    source: S({ environmentId: U, threadId: g(V), messageId: g(U_) }),
    records: w(v)
      .check(j(200))
      .pipe(xg(P_(Jv))),
  });
  var Zv = 64,
    Qv = /^[a-zA-Z][a-zA-Z0-9_-]*$/,
    $v = 128,
    ey = /^[a-zA-Z_][a-zA-Z0-9_]*$/,
    ty = L.check(j(Zv), Mg(Qv)),
    ny = ty.pipe(mg(`ProviderDriverKind`));
  Kh(ny);
  var q = ty.pipe(mg(`ProviderInstanceId`));
  S({ instanceId: q, driver: ny });
  var ry = w(
      S({
        name: L.check(j($v), Mg(ey)),
        value: y.pipe(O(m(``))),
        sensitive: x.pipe(O(m(!1))),
        valueRedacted: h(x),
      }),
    ),
    iy = S({
      driver: ny,
      displayName: g(L),
      accentColor: g(L),
      environment: h(ry),
      enabled: h(x),
      config: h(v),
    }),
    ay = T([
      S({ operation: _(`create`), instanceId: q, instance: iy }),
      S({ operation: _(`upsert`), instanceId: q, instance: iy }),
      S({ operation: _(`remove`), instanceId: q }),
    ]);
  C(q, iy);
  var oy = E([`true`, `false`, `unknown`]),
    sy = E([`unknown`, `nominal`, `fair`, `serious`, `critical`]),
    cy = S({
      source: E([
        `unknown`,
        `node-macos-shell`,
        `node-macos-native`,
        `node-linux`,
        `node-windows`,
        `electron-main`,
      ]),
      idle: oy,
      idleSeconds: D(b),
      locked: oy,
      suspended: x,
      onBattery: oy,
      lowPowerMode: oy,
      thermalState: sy,
      stale: x,
      updatedAt: N,
    }),
    ly = T([
      S({ type: _(`server-config`) }),
      S({ type: _(`provider-status`), instanceId: h(q) }),
      S({ type: _(`vcs-status`), cwd: y }),
      S({ type: _(`git-refs`), cwd: y }),
      S({ type: _(`diagnostics`) }),
      S({ type: _(`thread`), threadId: V }),
    ]),
    uy = E([`web`, `desktop-renderer`, `mobile`, `unknown`]),
    dy = L.check(j(128)),
    fy = S({
      environmentId: h(U),
      clientId: dy,
      clientKind: uy,
      visible: x,
      focused: x,
      recentlyInteracted: x,
      appState: h(E([`active`, `inactive`, `background`, `unknown`])),
      lowPowerMode: h(oy),
      batteryState: h(E([`unknown`, `unplugged`, `charging`, `full`])),
      networkType: h(y),
      scopes: w(ly),
      ttlMs: h(b),
      observedAt: N,
    }),
    py = S({
      hostPower: cy,
      leases: w(
        S({
          sessionId: K_,
          rpcClientId: q_,
          clientId: dy,
          clientKind: uy,
          visible: x,
          focused: x,
          recentlyInteracted: x,
          appState: h(E([`active`, `inactive`, `background`, `unknown`])),
          lowPowerMode: h(oy),
          batteryState: h(E([`unknown`, `unplugged`, `charging`, `full`])),
          networkType: h(y),
          scopes: w(ly),
          updatedAt: N,
          expiresAt: N,
        }),
      ),
      activeForegroundLeaseCount: b,
      activeScopeKeys: w(y),
      shouldRunOpportunisticWork: x,
      updatedAt: N,
    });
  E([`select`, `boolean`]);
  var my = S({ id: L, label: L, description: g(L), isDefault: g(x) }),
    hy = { id: L, label: L, description: g(L) },
    gy = T([
      S({
        ...hy,
        type: _(`select`),
        options: w(my),
        currentValue: g(L),
        promptInjectedValues: g(w(L)),
      }),
      S({ ...hy, type: _(`boolean`), currentValue: g(x) }),
    ]),
    _y = T([L, x]),
    vy = S({ id: L, value: _y }),
    yy = C(y, v).pipe(xg(w(vy), Td({ decode: (e) => m(xy(e)), encode: (e) => m(Sy(e)) }))),
    by = T([w(vy), yy]);
  function xy(e) {
    let t = [];
    for (let [n, r] of Object.entries(e)) {
      let e = typeof n == `string` ? n.trim() : ``;
      if (e.length !== 0) {
        if (typeof r == `string`) {
          let n = r.trim();
          n.length > 0 && t.push({ id: e, value: n });
        } else typeof r == `boolean` && t.push({ id: e, value: r });
      }
    }
    return t;
  }
  function Sy(e) {
    let t = {};
    for (let { id: n, value: r } of e) t[n] = r;
    return t;
  }
  var Cy = S({ optionDescriptors: g(w(gy)) }),
    wy = T([y, S({ slug: L, name: g(L), capabilities: g(Cy) })]);
  (ny.make(`codex`),
    ny.make(`claudeAgent`),
    ny.make(`cursor`),
    ny.make(`grok`),
    ny.make(`muse`),
    ny.make(`pi`),
    ny.make(`acpRegistry`),
    ny.make(`opencode`));
  var Ty = `gpt-6-luna`;
  (ny.make(`antigravity`), ny.make(`antigravity`), ny.make(`antigravity`));
  var Ey = L.check(j(128), Mg(/^[a-z0-9][a-z0-9._-]*$/)),
    Dy = E([`binary`, `npx`, `uvx`]),
    Oy = E([`sha256`, `registry`]),
    ky = S({ query: I.check(j(120)) }),
    Ay = y.check(j(2048)),
    jy = S({
      agents: w(
        S({
          id: Ey,
          name: L.check(j(160)),
          version: L.check(j(128)),
          description: y.check(j(1024)),
          authors: w(y.check(j(256))).check(j(16)),
          license: D(y.check(j(128))),
          website: D(Ay),
          repository: D(Ay),
          icon: D(Ay),
          distribution: Dy,
          integrity: Oy,
        }),
      ).check(j(20)),
    }),
    My = S({ agentId: Ey }),
    Ny = S({ agentId: Ey, version: L.check(j(128)), distribution: Dy, prepared: _(!0) }),
    Py = S({ agentId: Ey }),
    Fy = S({ agentId: Ey, removed: x }),
    Iy = L.check(j(256)),
    Ly = D(y.check(j(1024))),
    Ry = S({
      id: Iy,
      name: Iy,
      description: Ly,
      type: E([`agent`, `env_var`, `terminal`]),
      command: h(y.check(j(2048))),
      envVarNames: h(w(Iy).check(j(16))),
      link: h(Ay),
    }),
    zy = S({
      elicitationId: Iy,
      url: Ay,
      message: y.check(j(1024)),
      createdAt: h(y.check(j(128))),
      expiresAt: h(y.check(j(128))),
    }),
    By = S({ instanceId: q, elicitationId: Iy }),
    Vy = S({ accepted: x }),
    Hy = L.check(j(1024)),
    Uy = L.check(j(4096)),
    Wy = S({
      sessionId: Hy,
      cwd: Uy,
      additionalDirectories: w(Uy).check(j(32)),
      title: D(y.check(j(1024))),
      updatedAt: D(y.check(j(128))),
      importedThreadId: D(V),
    }),
    Gy = S({ instanceId: q, projectId: H, cursor: h(y.check(j(2048))) }),
    Ky = S({
      sessions: w(Wy).check(j(256)),
      nextCursor: D(y.check(j(2048))),
      canLoad: x,
      canResume: x,
      canDelete: x,
    }),
    qy = S({
      instanceId: q,
      projectId: H,
      sessionId: Hy,
      title: h(D(L.check(j(1024)))),
      updatedAt: h(D(L.check(j(128)))),
    }),
    Jy = S({ threadId: V, imported: x }),
    Yy = S({ instanceId: q, projectId: H, sessionId: Hy }),
    Xy = S({ deleted: _(!0) }),
    Zy = L.check(j(256)),
    Qy = L.check(j(64)),
    $y = S({
      providerId: Zy,
      supported: w(Qy).check(j(16)),
      required: x,
      current: D(S({ apiType: Qy, baseUrl: Ay })),
    }),
    eb = S({ instanceId: q, projectId: H }),
    tb = S({ providers: w($y).check(j(64)) }),
    nb = S({
      instanceId: q,
      projectId: H,
      providerId: Zy,
      apiType: Qy,
      baseUrl: Ay,
      headers: h(C(L.check(j(128)), y.check(j(8192)))),
    }),
    rb = S({ configured: _(!0) }),
    ib = S({ instanceId: q, projectId: H, providerId: Zy }),
    ab = S({ disabled: _(!0) }),
    ob = S({ instanceId: q }),
    sb = S({ loggedOut: _(!0) }),
    cb = S({ id: Iy, name: Iy, description: Ly });
  S({
    instanceId: q,
    ready: _(!0),
    icon: D(Ay),
    authMethods: w(Ry).check(j(32)),
    models: w(cb).check(j(256)),
    currentModelId: D(Iy),
    configOptions: w(gy).check(j(16)),
    sessionManagement: S({
      canList: x,
      canLoad: x,
      canResume: x,
      canLogout: x,
      canDelete: x,
      canConfigureProviders: x,
    }).pipe(
      O(
        m({
          canList: !1,
          canLoad: !1,
          canResume: !1,
          canLogout: !1,
          canDelete: !1,
          canConfigureProviders: !1,
        }),
      ),
    ),
  });
  var lb = E([
      `agent_not_configured`,
      `agent_not_found`,
      `archive_invalid`,
      `authentication_failed`,
      `checksum_mismatch`,
      `download_failed`,
      `install_failed`,
      `instance_not_found`,
      `logout_unsupported`,
      `logout_failed`,
      `probe_failed`,
      `project_not_found`,
      `registry_unavailable`,
      `runner_unavailable`,
      `session_import_failed`,
      `session_delete_unsupported`,
      `session_delete_failed`,
      `session_list_unsupported`,
      `session_resume_unsupported`,
      `providers_unsupported`,
      `providers_list_failed`,
      `provider_configuration_failed`,
      `unsupported_distribution`,
      `unsupported_platform`,
    ]),
    ub = class extends F()(`AcpRegistryOperationError`, {
      reason: lb,
      message: y,
      authMethods: h(w(Ry).check(j(32))),
      authAction: h(zy),
      cause: h(M()),
    }) {};
  ({ ...Rt });
  var db = {
      "~effect/Channel": { _Env: o, _InErr: o, _InElem: o, _OutErr: o, _OutElem: o },
      pipe() {
        return n(this, arguments);
      },
    },
    fb = (e) => {
      let t = Object.create(db);
      return ((t.transform = (t, n) => Xc(e(t, n), (e) => m(Vc(e)))), t);
    },
    pb = (e) => e.transform,
    mb = a(2, (e, t) => fb((n, r) => Yc(tl(pb(e)(n, r), t), tl(t)))),
    hb = (e) => e !== `GET` && e !== `HEAD` && e !== `OPTIONS` && e !== `TRACE`,
    gb = {
      Continue: 100,
      SwitchingProtocols: 101,
      Processing: 102,
      EarlyHints: 103,
      OK: 200,
      Ok: 200,
      Created: 201,
      Accepted: 202,
      NonAuthoritativeInformation: 203,
      NoContent: 204,
      ResetContent: 205,
      PartialContent: 206,
      MultiStatus: 207,
      AlreadyReported: 208,
      ImUsed: 226,
      MultipleChoices: 300,
      MovedPermanently: 301,
      Found: 302,
      SeeOther: 303,
      NotModified: 304,
      TemporaryRedirect: 307,
      PermanentRedirect: 308,
      BadRequest: 400,
      Unauthorized: 401,
      PaymentRequired: 402,
      Forbidden: 403,
      NotFound: 404,
      MethodNotAllowed: 405,
      NotAcceptable: 406,
      ProxyAuthenticationRequired: 407,
      RequestTimeout: 408,
      Conflict: 409,
      Gone: 410,
      LengthRequired: 411,
      PreconditionFailed: 412,
      PayloadTooLarge: 413,
      UriTooLong: 414,
      UnsupportedMediaType: 415,
      RangeNotSatisfiable: 416,
      ExpectationFailed: 417,
      ImATeapot: 418,
      MisdirectedRequest: 421,
      UnprocessableEntity: 422,
      Locked: 423,
      FailedDependency: 424,
      TooEarly: 425,
      UpgradeRequired: 426,
      PreconditionRequired: 428,
      TooManyRequests: 429,
      RequestHeaderFieldsTooLarge: 431,
      UnavailableForLegalReasons: 451,
      InternalServerError: 500,
      NotImplemented: 501,
      BadGateway: 502,
      ServiceUnavailable: 503,
      GatewayTimeout: 504,
      HttpVersionNotSupported: 505,
      VariantAlsoNegotiates: 506,
      InsufficientStorage: 507,
      LoopDetected: 508,
      NotExtended: 510,
      NetworkAuthenticationRequired: 511,
    },
    _b = (e) => gb[e],
    vb = `~effect/Stream`,
    yb = { _R: o, _E: o, _A: o },
    bb = function (e) {
      this.channel = e;
    };
  bb.prototype = {
    [vb]: yb,
    pipe() {
      return n(this, arguments);
    },
  };
  var xb = (e) => new bb(e),
    Sb = `~effect/Stream`,
    Cb = (e) => ye(e, Sb),
    wb = xb,
    Tb = a(2, (e, t) => wb(mb(e.channel, t))),
    Eb = `~effect/http-api/HttpApiSchema/Stream`;
  function Db(e) {
    let t = typeof e == `string` ? _b(e) : e;
    return (e) => e.annotate({ httpApiStatus: t });
  }
  var Ob = (e) => Qh(e) && ye(e, Eb),
    kb = (e) => Ob(e) && e._tag === `StreamSse`,
    Ab = `~effect/http-api/HttpApiSchema/WithHeaders`,
    jb = (e) => Qh(e) && ye(e, `~effect/http-api/HttpApiSchema/WithHeaders`);
  function Mb(e, t, n) {
    return Zh(e.ast, { [Ab]: Ab, schema: t, headers: n });
  }
  function Nb(e, t) {
    return e.annotate({
      "~httpApiEncoding": { _tag: t._tag, contentType: t.contentType ?? Pb(t._tag) },
    });
  }
  function Pb(e) {
    switch (e) {
      case `Multipart`:
        return `multipart/form-data`;
      case `Json`:
        return `application/json`;
      case `FormUrlEncoded`:
        return `application/x-www-form-urlencoded`;
      case `Uint8Array`:
        return `application/octet-stream`;
      case `Text`:
        return `text/plain`;
    }
  }
  function Fb(e) {
    return (t) => Nb(t, { _tag: `FormUrlEncoded`, ...e });
  }
  var Ib = (e) => {
      if (Kd(e) || Kd(Zp(e))) return !0;
      let t = e.encoding?.[0].to;
      return t !== void 0 && Kd(t);
    },
    Lb = vm(`~httpApiEncoding`),
    Rb = vm(`~httpApiWithHeaders`),
    zb = vm(`httpApiStatus`),
    Bb = { _tag: `Json`, contentType: `application/json` },
    Vb = { _tag: `FormUrlEncoded`, contentType: `application/x-www-form-urlencoded` };
  function Hb(e) {
    return Lb(e) ?? Bb;
  }
  function Ub(e, t) {
    return Lb(e) || (hb(t) ? Bb : Vb);
  }
  function Wb(e) {
    let t = Hb(e);
    if (t._tag === `Multipart`) throw Error(`Multipart is not supported in response`);
    return t;
  }
  function Gb(e) {
    return zb(e) ?? 200;
  }
  function Kb(e) {
    return jb(e) ? (zb(e.ast) ?? Gb(e.schema.ast)) : Gb(e.ast);
  }
  function qb(e) {
    return jb(e) && Lb(e.ast) === void 0 ? Wb(e.schema.ast) : Wb(e.ast);
  }
  function Jb(e) {
    return zb(e) ?? 500;
  }
  function Yb(e) {
    return jb(e) ? (zb(e.ast) ?? Jb(e.schema.ast)) : Jb(e.ast);
  }
  var Xb = E([`desktop-managed-local`, `loopback-browser`, `remote-reachable`, `unsafe-no-auth`]),
    Zb = E([`desktop-bootstrap`, `one-time-token`]),
    Qb = E([`browser-session-cookie`, `bearer-access-token`, `dpop-access-token`]),
    $b = `orchestration:read`,
    ex = `orchestration:operate`,
    tx = `settings:write`,
    nx = `providers:manage`,
    rx = `environment:maintain`,
    ix = `preview:operate`,
    ax = `diagnostics:read`,
    ox = `terminal:read`,
    sx = `terminal:operate`,
    cx = `source-control:write`,
    lx = `filesystem:read`,
    ux = `filesystem:write`,
    dx = `review:write`,
    fx = E([
      $b,
      ex,
      tx,
      nx,
      rx,
      ix,
      ax,
      ox,
      sx,
      lx,
      ux,
      dx,
      cx,
      `access:read`,
      `access:write`,
      `relay:read`,
      `relay:write`,
    ]),
    px = w(fx),
    mx = w(E(fx.literals.filter((e) => e !== dx))),
    hx = { scopes: px, permissions: h(P_(fx)) },
    gx = `urn:ietf:params:oauth:grant-type:token-exchange`,
    _x = `urn:ietf:params:oauth:token-type:access_token`,
    vx = `urn:t3:params:oauth:token-type:environment-bootstrap`,
    yx = S({
      policy: Xb,
      bootstrapMethods: w(Zb),
      sessionMethods: w(Qb),
      sessionCookieName: L,
      serverUpdateScope: h(_(rx)),
    }),
    bx = S({ credential: L }),
    xx = S({ authenticated: _(!0), ...hx, sessionMethod: Qb, expiresAt: N }),
    Sx = E([`desktop`, `mobile`, `tablet`, `bot`, `unknown`]);
  S({
    label: h(L),
    deviceType: h(Sx),
    os: h(L),
    osMajorVersion: h(A),
    deviceModel: h(L),
    surface: h(J_),
    webDeployment: h(Y_),
    browser: h(L),
    appVersion: h(L),
  });
  var Cx = S({
      grant_type: _(gx),
      subject_token: L,
      subject_token_type: _(vx),
      requested_token_type: _(_x),
      scope: h(L),
      client_label: h(L),
      client_device_type: h(Sx),
      client_os: h(L),
    }).pipe(Fb()),
    wx = S({
      access_token: L,
      issued_token_type: _(_x),
      token_type: E([`Bearer`, `DPoP`]),
      expires_in: b,
      scope: L,
    }),
    Tx = S({ ticket: L, expiresAt: N }),
    Ex = S({ id: L, credential: L, label: h(L), expiresAt: N }),
    Dx = S({ id: L, ...hx, subject: L, label: h(L), createdAt: N, expiresAt: N }),
    Ox = S({
      label: h(L),
      ipAddress: h(L),
      userAgent: h(L),
      deviceType: Sx,
      os: h(L),
      browser: h(L),
    }),
    kx = S({
      sessionId: K_,
      subject: L,
      ...hx,
      method: Qb,
      client: Ox,
      issuedAt: N,
      expiresAt: N,
      lastConnectedAt: D(N),
      connected: x,
      current: x,
    }),
    Ax = S({ pairingLinks: w(Dx), clientSessions: w(kx) }),
    jx = S({ version: _(1), revision: b, type: _(`snapshot`), payload: Ax }),
    Mx = S({ version: _(1), revision: b, type: _(`pairingLinkUpserted`), payload: Dx }),
    Nx = S({ version: _(1), revision: b, type: _(`pairingLinkRemoved`), payload: S({ id: L }) }),
    Px = class extends F()(`AuthAccessStreamError`, { message: y }) {},
    J = class extends F()(`EnvironmentAuthorizationError`, {
      message: y,
      requiredScope: fx,
      requiredPermission: h(y),
    }) {},
    Fx = T([
      jx,
      Mx,
      Nx,
      S({ version: _(1), revision: b, type: _(`clientUpserted`), payload: kx }),
      S({ version: _(1), revision: b, type: _(`clientRemoved`), payload: S({ sessionId: K_ }) }),
    ]),
    Ix = S({ id: L }),
    Lx = S({ sessionId: K_ }),
    Rx = S({ label: h(L), scopes: h(mx) }),
    zx = S({
      authenticated: x,
      auth: yx,
      scopes: h(px),
      permissions: hx.permissions,
      sessionMethod: h(Qb),
      expiresAt: h(N),
    }),
    Bx = E([`read-only`, `approval-required`, `auto-accept-edits`, `auto`, `full-access`]),
    Vx = S({
      resource: y,
      authorization_servers: w(y),
      scopes_supported: w(fx),
      bearer_methods_supported: w(_(`header`)),
      resource_name: y,
    }),
    Hx = S({
      issuer: y,
      authorization_endpoint: y,
      token_endpoint: y,
      registration_endpoint: y,
      response_types_supported: w(_(`code`)),
      grant_types_supported: w(_(`authorization_code`)),
      code_challenge_methods_supported: w(_(`S256`)),
      token_endpoint_auth_methods_supported: w(_(`none`)),
      scopes_supported: w(fx),
      authorization_response_iss_parameter_supported: x,
    }),
    Ux = S({ client_name: h(y), redirect_uris: h(w(y)), token_endpoint_auth_method: h(y) }),
    Wx = S({
      client_id: y,
      client_name: y,
      redirect_uris: w(y),
      grant_types: w(_(`authorization_code`)),
      response_types: w(_(`code`)),
      token_endpoint_auth_method: _(`none`),
    }).pipe(Db(201)),
    Gx = class extends w_(`AuthMcpRegistrationError`)(
      { error: E([`invalid_client_metadata`, `invalid_redirect_uri`]), error_description: y },
      { httpApiStatus: 400 },
    ) {
      get message() {
        return this.error_description;
      }
    },
    Kx = S({
      response_type: h(y),
      client_id: h(y),
      redirect_uri: h(y),
      code_challenge: h(y),
      code_challenge_method: h(y),
      state: h(y),
      resource: h(y),
    }),
    qx = S({
      clientName: y,
      redirectHost: y,
      environmentHost: y,
      csrfToken: h(y),
      oneClickAccess: h(w(Bx)),
    }),
    Jx = S({ redirectTo: y }),
    Yx = S({
      authorization: Kx,
      decision: T([
        Dg(`deny`, {}),
        Dg(`pairing-code`, { access: Bx, code: L }),
        Dg(`browser-session`, { access: Bx, csrfToken: y }),
      ]),
    }),
    Xx = class extends F()(`AuthMcpApprovalError`, { message: y }, { httpApiStatus: 400 }) {},
    Zx = S({
      grant_type: h(y),
      code: h(y),
      redirect_uri: h(y),
      client_id: h(y),
      code_verifier: h(y),
      resource: h(y),
    }).pipe(Fb()),
    Qx = S({ access_token: y, token_type: _(`Bearer`), expires_in: b, scope: y }),
    $x = class extends w_(`AuthMcpTokenError`)(
      {
        error: E([`invalid_request`, `invalid_client`, `invalid_grant`, `unsupported_grant_type`]),
        error_description: y,
      },
      { httpApiStatus: 400 },
    ) {
      get message() {
        return this.error_description;
      }
    },
    eS = `x-t3-orchestration-protocol`,
    tS = E([`darwin`, `linux`, `windows`, `unknown`]),
    nS = E([`arm64`, `x64`, `other`]),
    rS = E([`server`, `cloud`, `linux`, `desktop`, `laptop`, `mac-mini`, `mac-studio`]);
  Kh(rS);
  var iS = S({ os: tS, arch: nS, machine: j_(rS) }),
    aS = E([`local`, `worktree`]),
    oS = E([`recursive`, `top-level`, `none`]),
    sS = E([`boot-service`, `binary`, `respawn`, `desktop-app`]),
    cS = T([S({ kind: E([`npx`, `pnpm-dlx`, `bunx`]) }), S({ kind: _(`npm-global`), prefix: L })]),
    lS = E([`boot-service`, `binary`, `respawn`, `desktop-managed`]),
    uS = S({
      repositoryIdentity: x.pipe(O(m(!1))),
      connectionProbe: h(x),
      attachmentUploads: h(x),
      questionAttachments: h(x),
      fileAttachments: h(S({ maxUploadBytes: A.check(Hg(1)) })),
      pullRequests: h(x),
      pullRequestChecks: h(x),
      inlineMessageContext: h(x),
      requiredWorktreeBootstrap: h(x),
      threadSettlement: h(x),
      threadAutoSettlement: h(x),
      storageCleanup: h(x),
      storageCleanupRun: h(x),
      otlpEndpointCheck: h(x),
      projectWorktreeCleanup: h(x),
      worktreesDirectory: h(x),
      threadRestartContinuation: h(x),
      projectSettingsOverrides: h(x),
      threadSnooze: h(x),
      environmentThemes: h(x),
      usageLimitSources: h(x),
      usagePriceOverrides: h(x),
      usageModelMappings: h(x),
      threadPinning: h(x),
      threadPinReorder: h(x),
      threadActiveReorder: h(x),
      threadAutoSettleOptOut: h(x),
      threadTitleRegeneration: h(x),
      threadVisitedTracking: h(x),
      threadPullRequestLinking: h(x),
      serverResolvedCommandContext: h(x),
      threadPullRequests: h(x),
      threadPullRequestWatch: h(x),
      pullRequestStackActions: h(x),
      serverSelfUpdate: h(lS),
      serverInstallation: j_(cS),
      serverSelfUpdateProgress: h(x),
      serverUpdateThreadContinuation: h(x),
      agentActivityPublishing: h(x),
      projectCloneTracking: h(x),
      environmentIcon: h(x),
      desktopAppUpdate: h(x),
      serverBrowser: h(x),
    }),
    dS = S({
      environmentId: U,
      label: L,
      platform: iS,
      serverVersion: L,
      orchestrationProtocolVersion: h(A),
      capabilities: uS,
    }),
    fS = S({ source: _(`git-remote`), remoteName: L, remoteUrl: L }),
    pS = S({ canonicalKey: L, displayName: h(L) }),
    mS = S({
      canonicalKey: L,
      locator: fS,
      webUrl: h(L),
      rootPath: h(L),
      displayName: h(L),
      provider: h(L),
      owner: h(L),
      name: h(L),
      origin: h(pS),
    });
  (S({ environmentId: U, projectId: H }), S({ environmentId: U, threadId: V }));
  var hS = `~effect/http/HttpBody`,
    gS = `~effect/http/HttpBody/HttpBodyError`,
    _S = class extends $i(`HttpBodyError`) {
      [gS] = gS;
    },
    vS = class {
      [hS];
      constructor() {
        this[hS] = hS;
      }
      [yt]() {
        return this.toJSON();
      }
      toString() {
        return lt(this, { ignoreToString: !0 });
      }
    },
    yS = new (class extends vS {
      _tag = `Empty`;
      toJSON() {
        return { _id: `effect/HttpBody`, _tag: `Empty` };
      }
    })(),
    bS = class extends vS {
      _tag = `Uint8Array`;
      contentType;
      contentLength;
      text;
      _body;
      constructor(e, t, n, r) {
        (super(),
          (this._body = e),
          (this.text = r),
          (this.contentType = t),
          (this.contentLength = n));
      }
      get body() {
        return (this._body ??= CS(this.text));
      }
      toJSON() {
        return {
          _id: `effect/HttpBody`,
          _tag: `Uint8Array`,
          body:
            this.contentType.startsWith(`text/`) || this.contentType.endsWith(`json`)
              ? new TextDecoder().decode(this.body)
              : `Uint8Array(${this.body.length})`,
          contentType: this.contentType,
          contentLength: this.contentLength,
        };
      }
    },
    xS = new TextEncoder(),
    SS = globalThis.Buffer,
    CS = SS === void 0 ? (e) => xS.encode(e) : (e) => SS.from(e, `utf8`),
    wS = (e, t) => {
      if ((typeof e != `string` && (e = e === void 0 ? `` : String(e)), SS !== void 0))
        return new bS(void 0, t ?? `text/plain`, SS.byteLength(e, `utf8`), e);
      let n = xS.encode(e);
      return new bS(n, t ?? `text/plain`, n.length, e);
    },
    TS = (e, t) =>
      Wc({
        try: () => wS(JSON.stringify(e), t ?? `application/json`),
        catch: (e) => new _S({ reason: { _tag: `JsonError` }, cause: e }),
      }),
    ES = (e, t) => {
      let n = th(T_(e));
      return (e, r) =>
        n(e, t).pipe(
          Zc((e) => new _S({ reason: { _tag: `SchemaError`, issue: e }, cause: e })),
          qc((e) => TS(e, r)),
        );
    },
    DS = `~effect/ErrorReporter/ignore`;
  ({ ...xt });
  var OS = Object.getPrototypeOf(au),
    kS = () => Object.create(OS),
    AS = `~effect/http/HttpServerResponse`,
    jS = (e, t) => {
      if (e?.contentType) return e.contentType;
      if (e?.headers) return t[`content-type`];
    },
    MS = (e, t) => {
      let n = ES(e, t);
      return (e, t) => {
        let r = t?.headers ? ou(t.headers) : au;
        return Yc(n(e, jS(t, r)), (e) =>
          PS({
            status: t?.status ?? 200,
            statusText: t?.statusText,
            headers: r,
            cookies: t?.cookies,
            body: e,
          }),
        );
      };
    },
    NS = {
      ...Rt,
      [AS]: AS,
      [DS]: !0,
      toJSON() {
        return {
          _id: `HttpServerResponse`,
          status: this.status,
          statusText: this.statusText,
          headers: it(this.headers),
          cookies: this.cookies.toJSON(),
          body: this.body.toJSON(),
        };
      },
    },
    PS = (e, t) => {
      let n = Object.create(NS);
      if (
        ((n.status = e.status),
        (n.statusText = e.statusText),
        (n.cookies = e.cookies ?? Xl),
        (n.body = e.body ?? yS),
        n.body._tag !== `Empty` && (n.body.contentType || n.body.contentLength !== void 0))
      ) {
        let r = t !== void 0,
          i = r ? t : e.headers === void 0 || e.headers === au ? kS() : su({ ...e.headers });
        (n.body.contentType &&
          (!r || i[`content-type`] === void 0) &&
          (i[`content-type`] = n.body.contentType),
          n.body.contentLength !== void 0 &&
            (!r || i[`content-length`] === void 0) &&
            (i[`content-length`] = n.body.contentLength.toString()),
          (n.headers = i));
      } else n.headers = t ?? e.headers ?? au;
      return n;
    },
    FS = `~effect/http/HttpServerRespondable`,
    IS = new Uint8Array(256);
  for (let e of "!#$%&'*+-.^_`|~0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ")
    IS[e.charCodeAt(0)] = 1;
  var LS = new Uint8Array(256);
  LS[9] = 1;
  for (let e = 32; e <= 126; e++) LS[e] = 1;
  for (let e = 128; e <= 255; e++) LS[e] = 1;
  var RS = fr(`effect/http/HttpServerRequest`),
    zS = new WeakSet(),
    BS = (e) =>
      Gc((t) => {
        let n = Lr(t.context, RS);
        return (zS.add(n.source), e);
      }),
    VS = fr(`effect/http/HttpRouter`),
    HS = (e) => (e.endsWith(`/`) ? e.slice(0, -1) : e),
    US = a(2, (e, t) => ((t = HS(t)), e === `*` ? `${t}/*` : e === `/` ? t : t + e)),
    WS = `~effect/http/HttpRouter/Middleware`,
    GS = function () {
      return arguments.length === 0 ? KS : KS(arguments[0], arguments[1]);
    },
    KS = (e, t) =>
      t?.global
        ? ac(
            zc(function* () {
              let t = yield* VS,
                n = Fc(e) ? yield* e : e;
              yield* t.addGlobalMiddleware(n);
            }),
          )
        : new YS(Fc(e) ? ic(Yc(e, (e) => Tr(new Map([[JS, e]])))) : rc(Tr(new Map([[JS, e]])))),
    qS = 0,
    JS = `effect/http/HttpRouter/MiddlewareFn`,
    YS = class e {
      [WS] = {};
      layerFn;
      dependencies;
      constructor(e, t) {
        ((this.layerFn = e), (this.dependencies = t));
        let n = `effect/http/HttpRouter/Middleware-${++qS}`;
        this.layer = ic(
          zc({ self: this }, function* () {
            let e = yield* Qc(),
              t = [Ir(e, JS)];
            if (this.dependencies) {
              let n = yield* ec,
                r = Rr(e, Rs),
                i = yield* tc(this.dependencies, n, r);
              t.push(...ZS(i));
            }
            return Tr(new Map([[n, t]]));
          }),
        ).pipe(lc(this.layerFn));
      }
      layer;
      combine(t) {
        return new e(this.layerFn, this.dependencies ? uc(this.dependencies, t.layer) : t.layer);
      }
    },
    XS = new WeakMap(),
    ZS = (e) => {
      let t = XS.get(e);
      if (t) return t;
      let n = Vi(),
        r = 0;
      for (let [t, i] of e.mapUnsafe)
        t.startsWith(`effect/http/HttpRouter/Middleware-`) &&
          (n.push(i), i.length > r && (r = i.length));
      if (n.length === 0) t = [];
      else {
        let e = new Set();
        for (let t = r - 1; t >= 0; t--) for (let r of n) t < r.length && e.add(r[t]);
        t = Ii(e).reverse();
      }
      return (XS.set(e, t), t);
    };
  GS(BS).layer;
  function QS(e) {
    let t = e.toLowerCase().trim(),
      n = t.indexOf(`;`);
    return n === -1 ? t : t.slice(0, n).trim();
  }
  var $S = `~effect/http-api/HttpApiEndpoint`,
    eC = {
      [$S]: $S,
      pipe() {
        return n(this, arguments);
      },
      prefix(e) {
        return nC({ ...tC(this), path: US(this.path, e) });
      },
      middleware(e) {
        return nC({ ...tC(this), middlewares: new Set([...this.middlewares, e]) });
      },
      annotate(e, t) {
        return nC({ ...tC(this), annotations: Nr(this.annotations, e, t) });
      },
      annotateMerge(e) {
        return nC({ ...tC(this), annotations: Hr(this.annotations, e) });
      },
    },
    tC = (e) => ({
      identifier: e.identifier,
      path: e.path,
      method: e.method,
      params: e.params,
      query: e.query,
      headers: e.headers,
      payload: e.payload,
      success: e.success,
      error: e.error,
      annotations: e.annotations,
      middlewares: e.middlewares,
      disableCodecs: e.disableCodecs,
    });
  function nC(e) {
    function t() {}
    return (Object.setPrototypeOf(t, eC), Object.assign(t, e));
  }
  var rC = (e) => (t, n, r) => {
    let i = r?.disableCodecs ?? !1,
      a = i ? o : E_;
    return nC({
      identifier: t,
      path: n,
      method: e,
      params: iC(r?.params, a),
      query: iC(r?.query, a),
      headers: iC(r?.headers, a),
      payload: aC(r?.payload, e, i),
      success: sC(r?.success, e, i),
      error: lC(r?.error, i),
      annotations: Ar(),
      middlewares: new Set(),
      disableCodecs: i,
    });
  };
  function iC(e, t) {
    if (e !== void 0) return Qh(e) ? t(e) : t(S(e));
  }
  function aC(e, t, n) {
    let r = new Map();
    if (e === void 0) return r;
    let i = Array.isArray(e) ? e : Qh(e) ? [e] : [S(e).pipe(Fb())],
      a = n ? o : yC;
    for (let e of i) {
      let n = Ub(e.ast, t),
        i = QS(n.contentType),
        o = r.get(i);
      if (o) {
        if (o.encoding._tag !== n._tag)
          throw Error(`Multiple payload encodings for content-type: ${n.contentType}`);
        if (o.encoding._tag === `Multipart`)
          throw Error(`Multiple multipart payloads for content-type: ${n.contentType}`);
        o.schemas.push(a(e, t));
      } else r.set(i, { encoding: n, schemas: [a(e, t)] });
    }
    return r;
  }
  var oC = `effect/http-api/stream/failure`;
  function sC(e, t, n) {
    if (e === void 0) return new Set();
    let r = Li(e);
    return (uC(r, t), new Set(n ? r : r.map(cC)));
  }
  var cC = f((e) =>
    Ob(e) ? e : jb(e) ? Mb(e, Ob(e.schema) ? e.schema : vC(e.schema, qb(e)), E_(e.headers)) : _C(e),
  );
  function lC(e, t) {
    if (e === void 0) return new Set();
    let n = Li(e);
    for (let e of n)
      if (Ob(jb(e) ? e.schema : e))
        throw Error(`Streaming schemas are not supported in error responses`);
    return (fC(n, Yb), new Set(t ? n : n.map(cC)));
  }
  function uC(e, t) {
    let n = !1,
      r = new Map();
    for (let i of e) {
      let e = jb(i) ? i.schema : i,
        a = Kb(i);
      if (Ob(e)) {
        if ((pC(e, t), n)) throw Error(`Multiple streaming success responses are not supported`);
        n = !0;
        let i = dC(r, a);
        if (i.noContent)
          throw Error(`Cannot combine no-content and streaming success responses for status: ${a}`);
        if (i.bufferedContentTypes.has(QS(e.contentType)))
          throw Error(
            `Cannot combine buffered and streaming success responses for status ${a} and content-type: ${e.contentType}`,
          );
        r.set(a, { ...i, stream: e });
      } else {
        let t = dC(r, a),
          n = Ib(e.ast);
        if (t.stream !== void 0) {
          if (n)
            throw Error(
              `Cannot combine no-content and streaming success responses for status: ${a}`,
            );
          let e = qb(i);
          if (QS(e.contentType) === QS(t.stream.contentType))
            throw Error(
              `Cannot combine buffered and streaming success responses for status ${a} and content-type: ${e.contentType}`,
            );
        }
        (n || t.bufferedContentTypes.add(QS(qb(i).contentType)), (t.noContent = t.noContent || n));
      }
    }
    fC(e, Kb);
  }
  function dC(e, t) {
    let n = e.get(t);
    return (
      n === void 0 && ((n = { bufferedContentTypes: new Set(), noContent: !1 }), e.set(t, n)), n
    );
  }
  function fC(e, t) {
    let n = new Map();
    for (let r of e) {
      let e = t(r),
        i = Rb(r.ast),
        a = jb(r) ? r.schema : (i?.body ?? r),
        o = Ib(a.ast) ? `` : QS(Ob(a) ? a.contentType : qb(r).contentType),
        s = n.get(e);
      s === void 0 &&
        ((s = { headerContentType: void 0, plainContentTypes: new Set() }), n.set(e, s));
      let c = () =>
        Error(
          `Cannot combine a response with headers with another response for status ${e} and content-type: ${o || `<no content>`}`,
        );
      if (jb(r) || i !== void 0) {
        if (s.headerContentType !== void 0)
          throw Error(`Cannot declare multiple responses with headers for status ${e}`);
        if (s.plainContentTypes.has(o)) throw c();
        s.headerContentType = o;
      } else {
        if (s.headerContentType === o) throw c();
        s.plainContentTypes.add(o);
      }
    }
  }
  function pC(e, t) {
    if (t === `HEAD`) throw Error(`HEAD endpoints cannot declare streaming success responses`);
    if (kb(e) && mC(e.events.ast)) throw Error(`SSE event name is reserved: ${oC}`);
  }
  function mC(e) {
    return hC(Zp(e), new Set());
  }
  function hC(e, t) {
    if (t.has(e)) return !1;
    if ((t.add(e), Qd(e))) return e.types.some((e) => hC(e, t));
    if ($d(e)) return hC(e.thunk(), t);
    if (!Zd(e)) return !1;
    let n = e.propertySignatures.find((e) => e.name === `event`);
    return n !== void 0 && gC(n.type, t);
  }
  function gC(e, t) {
    if (t.has(e)) return !1;
    t.add(e);
    let n = Zp(e);
    return n === e
      ? Jd(e)
        ? e.literal === oC
        : Qd(e)
          ? e.types.some((e) => gC(e, t))
          : $d(e)
            ? gC(e.thunk(), t)
            : !1
      : gC(n, t);
  }
  function _C(e) {
    let t = Wb(e.ast),
      n = Rb(e.ast);
    return n === void 0
      ? vC(e, t)
      : e.annotate({
          "~httpApiWithHeaders": { ...n, bodyCodec: vC(n.body, t), headersCodec: E_(n.headers) },
        });
  }
  function vC(e, t) {
    switch (t._tag) {
      case `Json`:
        return T_(e);
      case `FormUrlEncoded`:
        return E_(e);
      case `Text`:
      case `Uint8Array`:
        return e;
    }
  }
  function yC(e, t) {
    switch (Ub(e.ast, t)._tag) {
      case `Json`:
        return T_(e);
      case `FormUrlEncoded`:
        return E_(e);
      case `Text`:
      case `Uint8Array`:
      case `Multipart`:
        return e;
    }
  }
  var bC = rC(`GET`),
    xC = rC(`POST`),
    SC = rC(`PUT`),
    CC = rC(`PATCH`),
    wC = rC(`DELETE`),
    TC = `~effect/http-api/HttpApi`,
    EC = {
      [TC]: TC,
      pipe() {
        return n(this, arguments);
      },
      add(...e) {
        let t = { ...this.groups };
        for (let n of e) Dt(t, n.identifier, n);
        return OC({ ...DC(this), groups: t });
      },
      addHttpApi(e) {
        let t = { ...this.groups };
        for (let n of Object.keys(e.groups)) {
          let r = e.groups[n];
          Dt(t, n, r.annotateMerge(Hr(e.annotations, r.annotations)));
        }
        return OC({ ...DC(this), groups: t });
      },
      prefix(e) {
        return OC({ ...DC(this), groups: ji(this.groups, (t) => t.prefix(e)) });
      },
      middleware(e) {
        return OC({ ...DC(this), groups: ji(this.groups, (t) => t.middleware(e)) });
      },
      annotate(e, t) {
        return OC({ ...DC(this), annotations: Nr(this.annotations, e, t) });
      },
      annotateMerge(e) {
        return OC({ ...DC(this), annotations: Hr(this.annotations, e) });
      },
    },
    DC = (e) => ({ identifier: e.identifier, groups: e.groups, annotations: e.annotations }),
    OC = (e) => {
      function t() {}
      return (Object.setPrototypeOf(t, EC), Object.assign(t, e));
    },
    kC = (e) => OC({ identifier: e, groups: {}, annotations: Ar() }),
    AC = `~effect/http-api/HttpApiGroup`,
    jC = {
      [AC]: AC,
      add(...e) {
        let t = { ...this.endpoints };
        for (let n of e) Dt(t, n.identifier, n);
        return NC({ ...MC(this), endpoints: t });
      },
      prefix(e) {
        return NC({ ...MC(this), endpoints: ji(this.endpoints, (t) => t.prefix(e)) });
      },
      middleware(e) {
        return NC({ ...MC(this), endpoints: ji(this.endpoints, (t) => t.middleware(e)) });
      },
      annotateMerge(e) {
        return NC({ ...MC(this), annotations: Hr(this.annotations, e) });
      },
      annotate(e, t) {
        return NC({ ...MC(this), annotations: Nr(this.annotations, e, t) });
      },
      annotateEndpointsMerge(e) {
        return NC({ ...MC(this), endpoints: ji(this.endpoints, (t) => t.annotateMerge(e)) });
      },
      annotateEndpoints(e, t) {
        return NC({ ...MC(this), endpoints: ji(this.endpoints, (n) => n.annotate(e, t)) });
      },
      pipe() {
        return n(this, arguments);
      },
    },
    MC = (e) => ({
      identifier: e.identifier,
      topLevel: e.topLevel,
      endpoints: e.endpoints,
      annotations: e.annotations,
    }),
    NC = (e) => {
      function t() {}
      return (
        Object.setPrototypeOf(t, jC),
        (t.key = `effect/http-api/HttpApiGroup/${e.identifier}`),
        Object.assign(t, e)
      );
    },
    PC = (e, t) =>
      NC({ identifier: e, topLevel: t?.topLevel ?? !1, endpoints: {}, annotations: Ar() }),
    FC = `~effect/http-api/HttpApiMiddleware`,
    IC = `~effect/http-api/HttpApiMiddleware/Security`,
    LC = () => (e, t) => {
      let n = Ct(),
        r;
      n !== 0 && (wt(2), (r = new globalThis.Error()), wt(n));
      class i extends fr()(e) {}
      let a = i;
      if (
        (Object.defineProperty(i, "stack", {
          get() {
            return r?.stack;
          },
        }),
        (a[FC] = FC),
        (a.error = RC(t?.error)),
        (a.requiredForClient = t?.requiredForClient ?? !1),
        t?.security !== void 0)
      ) {
        if (Object.keys(t.security).length === 0)
          throw Error(`HttpApiMiddleware.Service: security object must not be empty`);
        ((a[IC] = IC), (a.security = t.security));
      }
      return a;
    };
  function RC(e) {
    return e === void 0 ? new Set() : new Set(Array.isArray(e) ? e : [e]);
  }
  var zC = 12e4,
    BC = 10485760,
    VC = 83886080,
    HC = 52428800,
    UC = [`image/gif`, `image/jpeg`, `image/png`, `image/webp`],
    WC = new Set(UC);
  function GC(e) {
    return WC.has(e.toLowerCase());
  }
  var KC = 14e6,
    qC = L.check(j(128), Mg(/^[a-z0-9_-]+$/i)),
    JC = 32e3,
    YC = 1e4,
    XC = S({ x: R, y: R, width: z, height: z }),
    ZC = S({
      active: g(x),
      busy: g(x),
      checked: g(E([`on`, `off`, `mixed`])),
      editable: g(x),
      enabled: g(x),
      expanded: g(x),
      focused: g(x),
      selected: g(x),
      visible: g(x),
    }),
    QC = S({
      role: L.check(j(100)),
      name: h(L.check(j(1e3))),
      value: h(L.check(j(8e3))),
      description: h(L.check(j(2e3))),
      bounds: D(XC),
      state: h(ZC),
      actions: h(lg(w(L.check(j(100)))).check(j(32))),
      children: lg(w(fg(() => QC))).check(j(YC)),
    }),
    $C = T([
      S({ format: _(`flat-text`), text: L.check(j(JC)), truncated: x }),
      S({
        format: _(`element-tree`),
        coordinateSpace: _(`captured-image`),
        imageSize: S({ width: z, height: z }),
        truncated: x,
        root: QC,
      }),
    ]).check(
      k((e) => {
        if (e.format === `flat-text`) return;
        let t = 0,
          n = [e.root];
        for (; n.length > 0;) {
          let e = n.pop();
          if (((t += 1), t > 1e4)) return `Accessibility trees must not exceed ${YC} nodes.`;
          n.push(...e.children);
        }
        return (
          JSON.stringify(e).length <= 32e3 ||
          `Accessibility trees must not exceed 32000 serialized characters.`
        );
      }),
    ),
    ew = S({
      kind: _(`snap-shot`),
      capturedAt: B,
      appName: L.check(j(255)),
      windowTitle: I.check(j(1e3)),
      accessibleText: g(L.check(j(JC))),
      accessibility: g($C),
      appIdentifier: g(L.check(j(255))),
      appIconDataUrl: g(L.check(j(1e5), Mg(/^data:image\/png;base64,/i))),
    }),
    tw = S({
      type: _(`image`),
      id: qC,
      name: L.check(j(255)),
      mimeType: L.check(j(100), Mg(/^image\//i)),
      sizeBytes: R.check(Wg(BC)),
      source: g(ew),
    }),
    nw = Dg(`pasted-text`, {}),
    rw = S({
      type: _(`file`),
      id: qC,
      name: L.check(j(255)),
      mimeType: L.check(j(100)),
      sizeBytes: R.check(Hg(1), Wg(HC)),
      source: g(nw),
    }),
    iw = S({
      type: L.check(j(50), Mg(/^(?!(?:image|file)$)/)),
      id: qC,
      name: L.check(j(255)),
      mimeType: L.check(j(100)),
      sizeBytes: R,
    }),
    aw = S({
      id: g(qC),
      type: _(`image`),
      name: L.check(j(255)),
      mimeType: L.check(j(100), Mg(/^image\//i)),
      sizeBytes: R.check(Wg(BC)),
      dataUrl: L.check(j(KC)),
      source: g(ew),
    }),
    ow = T([tw, rw, iw]);
  function sw(e) {
    if (e.length > 100) return `You can attach up to 100 files per message or question response.`;
    if (e.reduce((e, t) => e + (t.type === `image` || GC(t.mimeType) ? t.sizeBytes : 0), 0) > VC)
      return `Images can total up to 80 MiB per message or question response. Use smaller images or send fewer at once.`;
  }
  var cw = S({ threadId: V, messageId: U_, attachments: w(T([aw])) }),
    lw = S({ attachments: w(ow) }),
    uw = class extends F()(`PersistChatAttachmentsError`, { message: y, cause: g(M()) }) {},
    dw = S({ fromTurnCount: R, toTurnCount: R }).check(
      k(
        (e) =>
          e.fromTurnCount <= e.toTurnCount ||
          new Nu({ message: `fromTurnCount must be less than or equal to toTurnCount` }),
        { identifier: `OrchestrationTurnDiffRange` },
      ),
    ),
    fw = dw.mapFields(jh({ threadId: V, diff: y }), { unsafePreserveChecks: !0 }),
    pw = dw.mapFields(jh({ threadId: V, ignoreWhitespace: h(x) }), { unsafePreserveChecks: !0 }),
    mw = fw,
    hw = S({ threadId: V, toTurnCount: R, ignoreWhitespace: h(x) }),
    gw = fw,
    _w = class extends F()(`OrchestrationGetTurnDiffError`, { message: L, cause: g(M()) }) {},
    vw = class extends F()(`OrchestrationGetFullThreadDiffError`, { message: L, cause: g(M()) }) {},
    yw = S({ instanceId: q, model: L, options: h(by) }),
    bw = S({ provider: g(v), instanceId: g(v), model: v, options: g(v) }).pipe(
      xg(
        yw,
        Td({
          decode: (e) => {
            let t = {
              instanceId:
                e.instanceId === void 0
                  ? typeof e.provider == `string`
                    ? e.provider
                    : void 0
                  : e.instanceId,
              model: e.model,
            };
            return (e.options !== void 0 && (t.options = e.options), m(t));
          },
          encode: (e) => {
            let t = { instanceId: e.instanceId, model: e.model };
            return (e.options !== void 0 && (t.options = e.options), m(t));
          },
        }),
      ),
    ),
    xw = E([`git`, `jj`, `unknown`]),
    Sw = S({
      source: E([`live-local`, `cached-local`, `cached-remote`, `explicit-remote`]),
      observedAt: N,
      expiresAt: m_(N),
    });
  (S({
    kind: xw,
    supportsWorktrees: x,
    supportsBookmarks: x,
    supportsAtomicSnapshot: x,
    supportsPushDefaultRemote: x,
    ignoreClassifier: E([`native`, `git-compatible-fallback`]),
  }),
    S({ kind: xw, rootPath: L, metadataPath: D(L), freshness: Sw }),
    S({ paths: w(L), truncated: x, freshness: Sw }),
    S({ remotes: w(S({ name: L, url: L, pushUrl: m_(L), isPrimary: x })), freshness: Sw }));
  var Cw = E([`authentication`, `not-found`, `rate-limited`, `command-failed`]),
    ww = class e extends F()(`VcsProcessSpawnError`, {
      operation: y,
      command: y,
      cwd: y,
      argumentCount: g(R),
      cause: M(),
    }) {
      get message() {
        return `VCS process failed to spawn in ${this.operation}: ${this.command} (${this.cwd})`;
      }
      static fromProcessSpawnError(t, n) {
        return new e({ ...t, cause: n.cause });
      }
    },
    Tw = class e extends F()(`VcsProcessExitError`, {
      operation: y,
      command: y,
      cwd: y,
      argumentCount: g(R),
      exitCode: b,
      detail: y,
      failureKind: g(Cw),
      retryable: g(x),
      stderrLength: g(R),
      stderrTruncated: g(x),
    }) {
      get message() {
        return `VCS process failed in ${this.operation}: ${this.command} (${this.cwd}) exited with ${this.exitCode} - ${this.detail}`;
      }
      static fromProcessExit(t, n, r, i) {
        let a =
          r === `authentication`
            ? `Authentication failed.`
            : r === `rate-limited`
              ? `API rate limit exceeded.`
              : r === `not-found`
                ? t.command === `glab`
                  ? `Merge request not found.`
                  : t.command === `gh` || t.command === `az`
                    ? `Pull request not found.`
                    : `VCS resource not found.`
                : `Process exited with a non-zero status.`;
        return new e({
          ...t,
          exitCode: n.exitCode,
          detail: a,
          failureKind: r,
          ...(i === !0 ? { retryable: !0 } : {}),
          stderrLength: n.stderr.length,
          stderrTruncated: n.stderrTruncated,
        });
      }
    },
    Ew = class e extends F()(`VcsProcessTimeoutError`, {
      operation: y,
      command: y,
      cwd: y,
      argumentCount: g(R),
      timeoutMs: b,
    }) {
      get message() {
        return `VCS process timed out in ${this.operation}: ${this.command} (${this.cwd}) after ${this.timeoutMs}ms`;
      }
      static fromProcessTimeoutError(t, n) {
        return new e({ ...t, timeoutMs: n.timeoutMs });
      }
    },
    Dw = { operation: y, command: y, cwd: y, argumentCount: g(R) },
    Ow = class extends F()(`VcsProcessStdinWriteError`, { ...Dw, stdinBytes: R, cause: M() }) {
      get message() {
        return `VCS process failed to write ${this.stdinBytes} bytes to stdin in ${this.operation}: ${this.command} (${this.cwd})`;
      }
    },
    kw = class extends F()(`VcsProcessOutputReadError`, {
      ...Dw,
      stream: E([`stdout`, `stderr`, `exitCode`]),
      cause: M(),
    }) {
      get message() {
        return `VCS process failed to read ${this.stream} in ${this.operation}: ${this.command} (${this.cwd})`;
      }
    },
    Aw = class extends F()(`VcsProcessOutputLimitError`, {
      ...Dw,
      stream: E([`stdout`, `stderr`]),
      maxBytes: R,
      observedBytes: R,
    }) {
      get message() {
        return `VCS process ${this.stream} produced ${this.observedBytes} bytes in ${this.operation}: ${this.command} (${this.cwd}), exceeding the ${this.maxBytes} byte limit`;
      }
    },
    jw = class extends F()(`VcsProcessMissingExitCodeError`, Dw) {
      get message() {
        return `VCS process completed without an exit code in ${this.operation}: ${this.command} (${this.cwd})`;
      }
    };
  T([Ow, kw, Aw, jw]);
  var Mw = T([
      ww,
      Tw,
      Ew,
      Ow,
      kw,
      Aw,
      jw,
      class extends F()(`VcsRepositoryDetectionError`, {
        operation: y,
        cwd: y,
        detail: y,
        cause: g(M()),
      }) {
        get message() {
          return `VCS repository detection failed in ${this.operation}: ${this.cwd} - ${this.detail}`;
        }
      },
      class extends F()(`VcsUnsupportedOperationError`, { operation: y, kind: xw, detail: y }) {
        get message() {
          return `VCS operation is unsupported for ${this.kind} in ${this.operation}: ${this.detail}`;
        }
      },
    ]),
    Nw = L.check(j(64), Mg(/^[a-z][a-z0-9-]*$/)).pipe(mg(`SourceControlProviderKind`)),
    Pw = S({ kind: Nw, name: L, baseUrl: y }),
    Fw = E([`open`, `closed`, `merged`]);
  (S({
    provider: Nw,
    number: z,
    title: L,
    url: y,
    baseRefName: L,
    headRefName: L,
    headSha: g(L),
    state: Fw,
    isDraft: g(x),
    closedAt: g(D(y)),
    mergedAt: g(D(y)),
    updatedAt: m_(N),
    isCrossRepository: g(x),
    headRepositoryNameWithOwner: g(D(L)),
    headRepositoryOwnerLogin: g(D(L)),
  }),
    S({ nameWithOwner: L, url: L, sshUrl: L }));
  var Iw = E([`private`, `public`]),
    Lw = E([`auto`, `ssh`, `https`]),
    Rw = S({ provider: Nw, nameWithOwner: L, url: L, sshUrl: L }),
    zw = S({ provider: Nw, repository: L, cwd: g(L) }),
    Bw = S({
      provider: g(Nw),
      repository: g(L),
      remoteUrl: g(L),
      destinationPath: L,
      protocol: g(Lw),
    }),
    Vw = S({ cwd: L, remoteUrl: L, repository: D(Rw) }),
    Hw = S({
      cwd: L,
      provider: Nw,
      repository: L,
      visibility: Iw,
      remoteName: g(L),
      protocol: g(Lw),
    }),
    Uw = E([`pushed`, `remote_added`]),
    Ww = S({
      repository: Rw,
      remoteName: L,
      remoteUrl: L,
      branch: L,
      upstreamBranch: g(L),
      status: Uw,
    }),
    Gw = E([`available`, `missing`]),
    Kw = S({
      status: E([`authenticated`, `unauthenticated`, `unknown`]),
      account: m_(L),
      host: m_(L),
      detail: m_(L),
      accounts: h(
        w(
          S({
            host: L,
            account: L,
            active: x,
            authenticated: x,
            error: h(L),
            environmentVariable: h(L),
          }),
        ),
      ),
    }),
    qw = { label: L, executable: g(L), status: Gw, version: m_(L), installHint: L, detail: m_(L) },
    Jw = S({ kind: xw, implemented: x, ...qw }),
    Yw = S({ kind: Nw, ...qw, auth: Kw }),
    Xw = S({ versionControlSystems: w(Jw), sourceControlProviders: w(Yw) }),
    Zw = class extends F()(`SourceControlProviderError`, {
      provider: Nw,
      operation: y,
      cwd: y,
      command: g(y),
      repository: g(y),
      reference: g(y),
      detail: y,
      cause: g(M()),
    }) {
      get message() {
        return `Source control provider ${this.provider} failed in ${this.operation}: ${this.detail}`;
      }
    },
    Qw = class extends F()(`SourceControlRepositoryError`, {
      provider: Nw,
      operation: y,
      detail: y,
      cause: g(M()),
    }) {
      get message() {
        return `Source control repository operation ${this.operation} failed for ${this.provider}: ${this.detail}`;
      }
    },
    $w = E([`all`, `reviewing`, `authored`]),
    eT = E([`open`, `closed`, `merged`]),
    tT = E([`all`, `open`, `closed`, `merged`]),
    nT = E([`approved`, `changes-requested`, `review-required`]),
    rT = L.check(j(200)),
    iT = w(rT).check(j(10)),
    aT = S({
      draft: g(E([`only`, `hide`])),
      review: g(E([`approved`, `changes-requested`, `review-required`, `none`])),
      checks: g(E([`passing`, `failing`])),
      labels: g(w(iT).check(j(10))),
      excludedLabels: g(iT),
      author: g(rT),
    }),
    oT = E([`passing`, `failing`, `pending`]),
    sT = E([`mergeable`, `conflicting`, `unknown`]),
    cT = E([`merge`, `squash`, `rebase`]),
    lT = E([
      `merge`,
      `ready`,
      `draft`,
      `close`,
      `reopen`,
      `update-branch`,
      `enable-auto-merge`,
      `disable-auto-merge`,
      `revert`,
      `approve-workflows`,
    ]),
    uT = E([`merge`, `rebase`]),
    dT = E([`up-to-date`, `behind`, `unknown`]),
    fT = S({ isBot: g(x), login: L, name: D(y), avatarUrl: D(y) }),
    pT = S({ name: L, color: D(y) }),
    mT = S({
      name: L,
      status: E([
        `pending`,
        `action-required`,
        `success`,
        `failure`,
        `skipped`,
        `neutral`,
        `cancelled`,
      ]),
      description: D(y),
      url: D(y),
      required: g(x),
    }),
    hT = E([`thumbs-up`, `thumbs-down`, `laugh`, `hooray`, `confused`, `heart`, `rocket`, `eyes`]),
    gT = S({ content: hT, count: z, actors: w(L), viewerHasReacted: x }),
    _T = S({
      id: L,
      kind: E([`issue-comment`, `review-comment`, `review`]),
      author: D(fT),
      body: y,
      createdAt: B,
      editedAt: g(D(B)),
      url: D(y),
      path: D(y),
      reviewState: D(y),
      reactions: g(w(gT)),
      canEdit: g(x),
    }),
    vT = E([`left`, `right`]),
    yT = E([`comment`, `approve`, `request-changes`]),
    bT = S({
      id: L,
      author: D(fT),
      body: y,
      createdAt: B,
      editedAt: g(D(B)),
      url: D(y),
      reactions: g(w(gT)),
      canEdit: g(x),
    }),
    xT = S({
      id: L,
      path: L,
      line: D(z),
      side: vT,
      isResolved: x,
      isOutdated: x,
      comments: w(bT),
      commentCount: g(R),
      nextCommentsCursor: g(L),
      canResolve: g(x),
    }),
    ST = E([`user`, `team`]),
    CT = S({ candidates: w(S({ ...fT.fields, id: L, kind: ST, isRequested: x })), truncated: x }),
    wT = S({ candidates: w(S({ ...pT.fields, description: D(y), isApplied: x })), truncated: x }),
    TT = S({
      oid: L,
      messageHeadline: y,
      committedDate: B,
      additions: g(R),
      deletions: g(R),
      authors: g(w(fT)),
    }),
    ET = S({ inlineComment: x, reply: x, resolve: x, verdicts: w(yT) }),
    DT = S({ changeRequest: x, comment: x }),
    OT = S({ request: x, listCandidates: x }),
    kT = E([`host`, `environment`]),
    AT = S({
      diff: x,
      comment: x,
      actions: w(lT),
      mergeMethods: w(cT),
      updateMethods: g(w(uT)),
      search: x,
      reactions: g(x),
      viewedFiles: g(kT),
      review: ET,
      reviewers: OT,
      edit: g(DT),
      stacks: g(x),
      stackActions: g(x),
      labels: g(x),
    }),
    jT = S({
      stackRebase: g(x),
      actions: w(lT),
      comment: x,
      resolve: x,
      verdicts: w(yT),
      requestReviewers: x,
      updateMethods: g(w(uT)),
      labels: g(x),
      editChangeRequest: g(x),
    }),
    MT = S({ merge: x, squash: x, rebase: x }),
    NT = S({ number: z, position: z, size: z, base: L }),
    PT = S({
      stack: g(NT),
      provider: Nw,
      host: L,
      projectId: H,
      projectTitle: L,
      repository: L,
      number: z,
      title: L,
      url: L,
      author: D(fT),
      headBranch: L,
      baseBranch: L,
      state: eT,
      isDraft: x,
      mergeability: sT,
      additions: R,
      deletions: R,
      createdAt: B,
      updatedAt: B,
      observedAt: g(Ng),
      viewerReviewRequested: x,
      labels: w(pT),
      reviewDecision: g(nT),
      checksState: g(oT),
    }),
    FT = C(L, L.check(j(4096))),
    IT = S({
      state: tT,
      involvement: g($w),
      filters: g(aT),
      projectId: g(H),
      projectIds: g(w(H).check(j(100))),
      host: g(L),
      limit: g(A.check(Gg({ minimum: 1, maximum: 500 }))),
      cursors: g(FT),
      query: g(L.check(j(200))),
    }),
    LT = S({
      host: L,
      kind: Nw,
      searchesOnHost: x,
      actions: g(w(lT)),
      projectCount: z,
      configured: x,
      detail: D(L),
    }),
    RT = S({ projectId: H, projectTitle: L, message: L }),
    zT = S({
      viewers: C(L, L),
      providers: w(LT),
      entries: w(PT),
      errors: w(RT),
      truncated: x,
      nextCursors: FT,
    }),
    BT = S({
      projectId: H,
      host: g(L),
      expectedAccountId: g(L),
      allowStale: g(x),
      repository: L,
      number: z,
    }),
    VT = S({ host: L }),
    HT = S({ accountId: L, host: L, provider: _(`github`), viewer: L }),
    UT = S({ accountId: L, host: L, provider: Nw, viewer: L, projectTitle: L, workspaceRoot: L }),
    WT = S({ threads: w(S({ id: V, projectId: H, title: y, archivedAt: D(B) })) }),
    GT = S({
      projectId: H,
      repository: L,
      number: z,
      title: L,
      url: L,
      author: D(fT),
      state: eT,
      isDraft: x,
      createdAt: B,
    }),
    KT = S({
      provider: Nw,
      projectId: H,
      repository: L,
      number: z,
      title: L,
      url: L,
      state: eT,
      isDraft: g(x),
      headBranch: L,
      baseBranch: L,
      closedAt: g(D(y)),
      mergedAt: g(D(y)),
      updatedAt: B,
      observedAt: g(Ng),
      author: g(D(fT)),
      additions: g(R),
      deletions: g(R),
      changedFiles: g(R),
      reviewDecision: g(D(nT)),
      checksState: g(D(oT)),
      mergeability: g(sT),
      stack: g(D(NT)),
    }),
    qT = S({
      id: L,
      number: z,
      url: L,
      base: L,
      layers: w(
        S({ number: z, title: g(y), isDraft: g(x), headSha: g(L), headBranch: L, state: eT }),
      ),
    }),
    JT = S({ projectId: H, repository: L, number: z, additions: R, deletions: R }),
    YT = S({ refs: w(BT).check(j(500)) }),
    XT = S({ stats: w(JT) }),
    ZT = S({ reference: g(BT), filesViewedOnly: g(x) }),
    QT = S({ reference: BT, state: eT }),
    $T = S({
      provider: Nw,
      capabilities: AT,
      viewerPermissions: jT,
      projectId: H,
      projectTitle: L,
      workspaceRoot: L,
      repository: L,
      number: z,
      title: L,
      body: y,
      url: L,
      author: D(fT),
      state: eT,
      isDraft: x,
      mergeability: sT,
      additions: R,
      deletions: R,
      changedFiles: R,
      headBranch: L,
      headRepositoryNameWithOwner: g(D(L)),
      headSha: g(L),
      baseBranch: L,
      createdAt: B,
      updatedAt: B,
      observedAt: g(Ng),
      mergedAt: D(B),
      closedAt: D(B),
      reviewers: w(fT),
      labels: w(pT),
      checks: w(mT),
      mergeCapabilities: MT,
      viewer: g(L),
      baseComparison: g(dT),
      behindBy: g(R),
      autoMergeEnabled: g(x),
      autoMergeMethod: g(cT),
      workflowApprovalsRequired: g(R),
    }),
    eE = S({ state: eT, checks: w(mT) }),
    tE = S({
      author: g(D(fT)),
      reviewers: g(w(fT)),
      comments: w(_T),
      commentCount: R,
      commentsTruncated: x,
      reviewThreadsTruncated: g(x),
      reviewThreads: w(xT),
      commits: w(TT),
      reactions: g(w(gT)),
    });
  S({ ...$T.fields, ...tE.fields, author: D(fT), reviewers: w(fT) });
  var nE = S({ ...BT.fields, cursor: g(L), commit: g(L) }),
    rE = S({ path: L, additions: b, deletions: b }),
    iE = S({ patch: y, truncated: x, nextCursor: D(L), omittedFileStats: g(w(rE)) }),
    aE = S({
      ...BT.fields,
      commit: g(L),
      changeType: E([`change`, `rename-pure`, `rename-changed`, `new`, `deleted`]),
      oldPath: L,
      newPath: L,
    }),
    oE = S({ oldContents: y, newContents: y }),
    sE = y.check(Yg(), j(4096)),
    cE = S({
      files: w(S({ path: sE, state: E([`unviewed`, `viewed`, `dismissed`]) })),
      truncated: x,
    }),
    lE = 500,
    uE = S({ ...BT.fields, files: w(S({ path: sE, viewed: x })).check(j(lE)) }),
    dE = S({ number: z, headSha: L }),
    fE = S({
      stackNumber: g(z),
      expectedStackHeads: g(w(dE)),
      ...BT.fields,
      action: lT,
      mergeMethod: g(cT),
      updateMethod: g(uT),
    }),
    pE = y.check(Yg()).check(j(65536)),
    mE = S({ ...BT.fields, body: pE }),
    hE = S({ ...BT.fields, title: g(L.check(j(1024))), body: g(y.check(j(65536))) }),
    gE = S({ ...BT.fields, commentId: L, kind: E([`issue-comment`, `review-comment`]), body: pE }),
    _E = T([
      S({ kind: _(`added`), newLine: z }),
      S({ kind: _(`deleted`), oldLine: z }),
      S({ kind: _(`context`), oldLine: z, newLine: z, side: vT }),
    ]),
    vE = S({ path: L, oldPath: g(L), position: _E, body: pE }),
    yE = S({ ...BT.fields, verdict: yT, body: y.check(j(65536)), comments: w(vE) }),
    bE = S({ ...BT.fields, threadId: L, cursor: L }),
    xE = S({ comments: w(bT), nextCursor: D(L) }),
    SE = S({ ...BT.fields, threadId: L, body: pE }),
    CE = S({ ...BT.fields, threadId: L, resolved: x }),
    wE = S({ ...BT.fields, subjectId: g(L), content: hT, reacted: x }),
    TE = S({
      ...BT.fields,
      reviewers: w(S({ id: L, kind: ST })).check(qg(1), j(25)),
      requested: x,
    }),
    EE = S({ ...BT.fields, labels: w(L).check(qg(1), j(25)), applied: x }),
    DE = E([`cli-missing`, `cli-unauthenticated`, `provider-unsupported`]),
    OE = new Map([
      [
        `github`,
        {
          missing:
            "No GitHub credential on the server. Set GH_TOKEN, or install the GitHub CLI (https://cli.github.com/) and run `gh auth login`.",
          unauthenticated:
            "GitHub has no working credential for this host. Run `gh auth login`, or check the account and hosts in Settings → Source Control.",
        },
      ],
      [
        `forgejo`,
        {
          missing:
            "Install Forgejo CLI (`fj` 0.6 or later) from https://codeberg.org/forgejo-contrib/forgejo-cli or Gitea CLI (`tea` 0.16 or later) from https://gitea.com/gitea/tea to browse Forgejo pull requests.",
          unauthenticated:
            "Authenticate your Forgejo or Gitea server with `fj --host <server-url> auth add-token` on the S5 Code server. If fj is missing or unconfigured for that server, use `tea login add`. A configured fj account must be repaired with fj.",
        },
      ],
      [
        `gitlab`,
        {
          missing:
            "GitLab CLI (`glab`) is required to browse change requests on this host. Install it from https://gitlab.com/gitlab-org/cli and reload.",
          unauthenticated: "GitLab CLI is not authenticated. Run `glab auth login` and retry.",
        },
      ],
      [
        `azure-devops`,
        {
          missing:
            "Azure CLI (`az`) with the Azure DevOps extension is required. Install `az`, then run `az extension add --name azure-devops`.",
          unauthenticated: "Azure CLI is not signed in. Run `az login` and retry.",
        },
      ],
      [
        `bitbucket`,
        {
          missing: `Bitbucket needs API credentials on the server. Add them in Settings → Source Control.`,
          unauthenticated: `Bitbucket rejected the configured credentials. Check them in Settings → Source Control.`,
        },
      ],
    ]),
    kE = class e extends F()(
      `PullRequestUnavailableError`,
      { reason: DE, provider: g(Nw), cause: g(M()) },
      { httpApiStatus: 503 },
    ) {
      [FS]() {
        return MS(e)(this, { status: 503 });
      }
      get message() {
        let e = this.provider === void 0 ? void 0 : OE.get(this.provider);
        switch (this.reason) {
          case `cli-missing`:
            return e?.missing ?? `The tool this host is read through is not installed or set up.`;
          case `cli-unauthenticated`:
            return e?.unauthenticated ?? `This host has no working credentials.`;
          case `provider-unsupported`:
            return `Change requests cannot be browsed for this project's host yet.`;
        }
      }
    },
    AE = class e extends F()(
      `PullRequestOperationError`,
      { operation: y, detail: L, reason: g(_(`not-found`)), cause: g(M()) },
      { httpApiStatus: 502 },
    ) {
      [FS]() {
        return MS(e)(this, { status: 502 });
      }
      get message() {
        return `Pull request operation ${this.operation} failed: ${this.detail}`;
      }
    },
    jE = S({ projectId: H, repository: L, number: z, url: L }),
    ME = E([`manual`, `created`, `agent`, `stack`, `stack-dismissed`]),
    NE = S({
      state: eT,
      title: L,
      headBranch: L,
      baseBranch: L,
      isDraft: x,
      updatedAt: D(B),
      syncedAt: B,
      closedAt: g(D(y)),
      mergedAt: g(D(y)),
      author: g(D(fT)),
      additions: g(R),
      deletions: g(R),
      changedFiles: g(R),
      reviewDecision: g(D(nT)),
      checksState: g(D(oT)),
      mergeability: g(sT),
    }),
    PE = S({ number: z, headBranch: L, state: eT }),
    FE = S({ kind: _(`native`), id: L, number: z, url: L, base: L, layers: w(PE) }),
    IE = S({ host: L, repository: L, number: z }),
    LE = S({
      startedAt: B,
      headSha: D(L),
      failedChecks: w(L),
      passed: x,
      passedChecks: w(L).pipe(O(m([]))),
      remarksThrough: B,
      remarkIds: w(L),
      conflicting: x,
      wakes: R,
    }),
    RE = S({
      ...IE.fields,
      url: L,
      source: ME,
      linkedAt: B,
      snapshot: D(NE),
      stack: D(FE),
      watch: g(LE),
    }),
    zE = E([`untrusted`, `on-failure`, `on-request`, `never`]),
    BE = E([`read-only`, `workspace-write`, `danger-full-access`]),
    VE = E([`approval-required`, `auto-accept-edits`, `auto`, `full-access`]),
    HE = `full-access`,
    UE = E([`default`, `plan`]),
    WE = E([`command`, `file-read`, `file-change`, `mcp-elicitation`, `permission`]);
  E([`buffered`, `streaming`]);
  var GE = E([`accept`, `acceptForSession`, `acceptAlways`, `decline`, `cancel`]),
    KE = S({ decision: GE, label: L, warning: g(L) }),
    qE = C(y, v),
    JE = C(y, w(T([tw, rw])).pipe(pg(j(100)))),
    YE = S({
      requestId: L,
      questionTextById: g(C(y, y)),
      answers: qE,
      attachmentsByQuestionId: JE,
    }),
    XE = 200,
    ZE = 500,
    QE = 512,
    $E = 512,
    eD = E([`play`, `test`, `lint`, `configure`, `build`, `debug`]),
    tD = S({
      id: L,
      name: L,
      command: L,
      icon: eD,
      runOnWorktreeCreate: x,
      runOnSettle: g(x),
      async: g(x),
      previewUrl: g(L),
      autoOpenPreview: g(x),
    }),
    nD = E([
      `gray`,
      `red`,
      `orange`,
      `amber`,
      `yellow`,
      `lime`,
      `green`,
      `emerald`,
      `teal`,
      `cyan`,
      `sky`,
      `blue`,
      `indigo`,
      `violet`,
      `purple`,
      `fuchsia`,
      `pink`,
      `rose`,
    ]),
    rD = L.check(j(64), Mg(/^[a-z0-9]+(?:-[a-z0-9]+)*$/)),
    iD = L.check(j(32)),
    aD = L.check(j(32), Mg(/^[\p{L}\p{N}][\p{L}\p{N}\p{M}\u200c\u200d]*$/u)),
    oD = S({ kind: _(`lucide`), name: rD, color: nD }),
    sD = S({ kind: _(`emoji`), emoji: iD }),
    cD = S({ kind: _(`monogram`), text: aD, color: nD }),
    lD = L.check(j(1024), Mg(/\.(?:avif|gif|icns|ico|jpe?g|png|svg|webp)$/i)),
    uD = T([oD, sD, cD]),
    dD = [S({ ...oD.fields, monogramText: g(aD), monogram: g(aD) }), sD, cD],
    fD = (e) => {
      if (e.kind !== `lucide`) return e;
      let t = e.monogramText ?? e.monogram;
      return t === void 0
        ? { kind: `lucide`, name: e.name, color: e.color }
        : { kind: `monogram`, text: t, color: e.color };
    },
    pD = T(dD).pipe(xg($h(uD), Ed({ decode: fD, encode: (e) => e }))),
    mD = I_(dD, `kind`).pipe(
      xg(D($h(uD)), Ed({ decode: (e) => (R_(e) ? null : fD(e)), encode: (e) => e })),
    ),
    hD = S({
      id: H,
      title: L,
      workspaceRoot: L,
      repositoryIdentity: g(D(mS)),
      faviconPath: g(D(L)),
      projectIcon: g(D(mD)),
      defaultModelSelection: D(bw),
      defaultThreadEnvMode: g(D(aS)),
      autoPull: g(x),
      scripts: w(tD),
      createdAt: B,
      updatedAt: B,
      deletedAt: D(B),
    }),
    gD = S({ projects: w(hD), updatedAt: B });
  T([
    S({ type: _(`project.upserted`), project: hD }),
    S({ type: _(`project.deleted`), projectId: H, deletedAt: B }),
  ]);
  var _D = S({
      title: L,
      workspaceRoot: L,
      createWorkspaceRootIfMissing: g(x),
      defaultModelSelection: g(D(bw)),
      scripts: g(w(tD)),
    }),
    vD = S({
      title: g(L),
      workspaceRoot: g(L),
      defaultModelSelection: g(D(bw)),
      autoPull: g(x),
      projectIcon: g(D(uD)),
      faviconPath: g(D(L)),
      defaultThreadEnvMode: g(D(aS)),
      scripts: g(w(tD)),
    }),
    yD = T([
      S({ type: _(`project.create`), commandId: W, projectId: H, ..._D.fields }),
      S({ type: _(`project.update`), commandId: W, projectId: H, ...vD.fields }),
      S({ type: _(`project.delete`), commandId: W, projectId: H, force: g(x) }),
    ]),
    bD = class extends F()(`ProjectMutationError`, { commandId: W, message: y, cause: g(M()) }) {},
    xD = E([`file`, `directory`]),
    SD = S({
      cwd: L,
      query: I.check(j(256)),
      limit: z.check(Wg(XE)),
      kind: g(xD),
      imageOnly: g(x),
    }),
    CD = S({ path: L, kind: xD, ignored: g(x) }),
    wD = S({ entries: w(CD), truncated: x }),
    TD = S({
      cwd: L,
      query: y.check(Yg(), j(256)),
      limit: z.check(Wg(ZE)),
      caseSensitive: x,
      wholeWord: x,
      useRegex: x,
    }),
    ED = S({
      matches: w(
        S({ path: L, lineNumber: z, lineContent: y, matchRanges: w(S({ start: R, end: R })) }),
      ),
      truncated: x,
      regexFallbackError: g(y),
    }),
    DD = S({ cwd: L, directoryPath: g(I) }),
    OD = S({ entries: w(CD), truncated: x }),
    kD = E([
      `workspace_root_not_found`,
      `workspace_root_create_failed`,
      `workspace_root_stat_failed`,
      `workspace_root_not_directory`,
      `search_index_create_failed`,
      `search_index_scan_timed_out`,
      `search_index_search_failed`,
      `directory_list_failed`,
    ]);
  function AD(e) {
    if (`message` in e) return typeof e.message == `string` ? e.message : void 0;
  }
  var jD = class extends F()(`ProjectSearchEntriesError`, {
      cwd: g(L),
      queryLength: g(R),
      limit: g(z),
      failure: g(kD),
      normalizedCwd: g(L),
      timeout: g(L),
      detail: g(L),
      message: L,
      cause: g(M()),
    }) {
      constructor(e) {
        super({ ...e, message: AD(e) ?? `Failed to search workspace entries in '${e.cwd}'.` });
      }
    },
    MD = class extends F()(`ProjectSearchContentsError`, {
      cwd: g(L),
      queryLength: g(R),
      limit: g(z),
      failure: g(kD),
      normalizedCwd: g(L),
      timeout: g(L),
      detail: g(L),
      message: L,
      cause: g(M()),
    }) {
      constructor(e) {
        super({ ...e, message: AD(e) ?? `Failed to search workspace contents in '${e.cwd}'.` });
      }
    },
    ND = class extends F()(`ProjectListEntriesError`, {
      cwd: g(L),
      failure: g(kD),
      normalizedCwd: g(L),
      timeout: g(L),
      detail: g(L),
      message: L,
      cause: g(M()),
    }) {
      constructor(e) {
        super({ ...e, message: AD(e) ?? `Failed to list workspace entries in '${e.cwd}'.` });
      }
    },
    PD = S({ cwd: L, relativePath: L.check(j($E)) }),
    FD = S({ relativePath: L, contents: y, byteLength: R, truncated: x }),
    ID = E([
      `workspace_path_outside_root`,
      `resolved_path_outside_root`,
      `path_not_file`,
      `binary_file`,
      `operation_failed`,
    ]),
    LD = E([
      `realpath-workspace-root`,
      `realpath-target`,
      `open`,
      `stat`,
      `read`,
      `close`,
      `make-directory`,
      `write-file`,
    ]),
    RD = class extends F()(`ProjectReadFileError`, {
      cwd: g(L),
      relativePath: g(L),
      failure: g(ID),
      resolvedPath: g(L),
      resolvedWorkspaceRoot: g(L),
      operation: g(LD),
      operationPath: g(L),
      message: L,
      cause: g(M()),
    }) {
      constructor(e) {
        super({
          ...e,
          message: AD(e) ?? `Failed to read workspace file '${e.relativePath}' in '${e.cwd}'.`,
        });
      }
    },
    zD = S({ cwd: L, relativePath: L.check(j(QE)), contents: y }),
    BD = S({ relativePath: L }),
    VD = S({ projectId: H }),
    HD = S({ name: L.check(j(200)) }),
    UD = S({ projectId: H, workspaceRoot: L, commitError: h(L) }),
    WD = class extends F()(`ProjectWriteFileError`, {
      cwd: g(L),
      relativePath: g(L),
      failure: g(ID),
      resolvedPath: g(L),
      resolvedWorkspaceRoot: g(L),
      operation: g(LD),
      operationPath: g(L),
      message: L,
      cause: g(M()),
    }) {
      constructor(e) {
        super({
          ...e,
          message: AD(e) ?? `Failed to write workspace file '${e.relativePath}' in '${e.cwd}'.`,
        });
      }
    },
    GD = S({
      id: H,
      title: L,
      workspaceRoot: L,
      repositoryIdentity: g(D(mS)),
      defaultModelSelection: D(bw),
      defaultThreadEnvMode: g(D(aS)),
      autoPull: g(x),
      faviconPath: g(D(L)),
      projectIcon: g(D(mD)),
      scripts: w(tD),
      createdAt: B,
      updatedAt: B,
    }),
    KD = y.pipe(mg(`UsageLimitSourceId`)),
    qD = S({
      id: L,
      kind: E([`session`, `weekly`, `monthly`, `other`]),
      label: L,
      usedPercent: b.check(Gg({ minimum: 0, maximum: 100 })),
      resetsAt: g(B),
      windowDurationMins: g(R),
    }),
    JD = S({ availableCount: R, nextExpiresAt: g(B), nextCreditId: g(L) }),
    YD = S({
      checkedAt: B,
      windows: P_(qD),
      credentialFingerprint: g(L),
      resetCredits: g(JD),
      externalUsage: g(S({ label: L, url: L })),
      unavailable: g(S({ reason: E([`unsupported`, `probeFailed`]), message: g(L) })),
    }),
    XD = S({ windows: w(qD) }),
    ZD = S({ id: L, driver: ny, email: g(L), plan: g(L), workspaceId: g(L), usageLimits: YD }),
    QD = P_(
      S({ id: KD, kind: _(`cliproxy`), label: L, checkedAt: B, accounts: P_(ZD), error: g(L) }),
    ),
    $D = S({ sourceId: KD, accountId: L, creditId: L }),
    eO = T([S({ instanceId: q }), $D]),
    tO = class extends F()(`UsageLimitSourceError`, { detail: y }) {
      get message() {
        return this.detail;
      }
    },
    nO = S({
      outcome: E([`reset`, `nothingToReset`, `noCredit`, `alreadyRedeemed`]),
      warning: g(L),
    });
  S({
    createdAt: B,
    accounts: w(
      S({
        id: L,
        driver: ny,
        label: L,
        plan: g(L),
        email: g(L),
        sourceLabel: g(L),
        instanceId: g(q),
        resetCreditInput: g(eO),
        displayName: g(y),
        accentColor: g(y),
        limits: YD,
      }),
    ),
    notices: w(y),
  });
  var Y = L,
    rO = C(y, v),
    iO = S({
      source: T([
        _(`codex.app-server.notification`),
        _(`codex.app-server.request`),
        _(`codex.eventmsg`),
        _(`claude.sdk.message`),
        _(`claude.sdk.permission`),
        _(`codex.sdk.thread-event`),
        _(`opencode.sdk.event`),
        _(`acp.jsonrpc`),
        ng([`acp.`, y, `.extension`]),
      ]),
      method: g(Y),
      messageType: g(Y),
      payload: v,
    }),
    aO = Y,
    oO = S({ providerTurnId: g(Y), providerItemId: g(X_), providerRequestId: g(aO) }),
    sO = E([`starting`, `ready`, `running`, `waiting`, `stopped`, `error`]),
    cO = E([`active`, `idle`, `archived`, `closed`, `compacted`, `error`]),
    lO = E([`completed`, `failed`, `interrupted`, `cancelled`]),
    uO = E([`pending`, `inProgress`, `completed`]),
    dO = E([`inProgress`, `completed`, `failed`, `declined`]),
    fO = E([
      `assistant_text`,
      `reasoning_text`,
      `reasoning_summary_text`,
      `plan_text`,
      `command_output`,
      `file_change_output`,
      `unknown`,
    ]),
    pO = E([`graceful`, `error`]),
    mO = E([
      `provider_error`,
      `transport_error`,
      `permission_error`,
      `validation_error`,
      `unknown`,
    ]),
    hO = [
      `command_execution`,
      `file_change`,
      `mcp_tool_call`,
      `dynamic_tool_call`,
      `collab_agent_tool_call`,
      `web_search`,
      `image_view`,
    ];
  E(hO);
  var gO = E([
      `user_message`,
      `assistant_message`,
      `reasoning`,
      `plan`,
      ...hO,
      `review_entered`,
      `review_exited`,
      `context_compaction`,
      `error`,
      `unknown`,
    ]),
    _O = E([
      `command_execution_approval`,
      `file_read_approval`,
      `file_change_approval`,
      `apply_patch_approval`,
      `exec_command_approval`,
      `mcp_elicitation_approval`,
      `permission_approval`,
      `tool_user_input`,
      `dynamic_tool_call`,
      `auth_tokens_refresh`,
      `unknown`,
    ]),
    vO = _(`session.started`),
    yO = _(`session.configured`),
    bO = _(`session.state.changed`),
    xO = _(`session.exited`),
    SO = _(`thread.started`),
    CO = _(`thread.state.changed`),
    wO = _(`thread.metadata.updated`),
    TO = _(`thread.token-usage.updated`),
    EO = _(`thread.realtime.started`),
    DO = _(`thread.realtime.item-added`),
    OO = _(`thread.realtime.audio.delta`),
    kO = _(`thread.realtime.error`),
    AO = _(`thread.realtime.closed`),
    jO = _(`turn.started`),
    MO = _(`turn.completed`),
    NO = _(`turn.aborted`),
    PO = _(`turn.plan.updated`),
    FO = _(`turn.proposed.delta`),
    IO = _(`turn.proposed.completed`),
    LO = _(`turn.diff.updated`),
    RO = _(`item.started`),
    zO = _(`item.updated`),
    BO = _(`item.completed`),
    VO = _(`content.delta`),
    HO = _(`request.opened`),
    UO = _(`request.resolved`),
    WO = _(`user-input.requested`),
    GO = _(`user-input.resolved`),
    KO = _(`task.started`),
    qO = _(`task.progress`),
    JO = _(`task.updated`),
    YO = _(`task.completed`),
    XO = _(`hook.started`),
    ZO = _(`hook.progress`),
    QO = _(`hook.completed`),
    $O = _(`tool.progress`),
    ek = _(`tool.summary`),
    tk = _(`auth.status`),
    nk = _(`account.updated`),
    rk = _(`account.rate-limits.updated`),
    ik = _(`mcp.status.updated`),
    ak = _(`mcp.oauth.completed`),
    ok = _(`model.rerouted`),
    sk = _(`config.warning`),
    ck = _(`deprecation.notice`),
    lk = _(`files.persisted`),
    uk = _(`tool.denied`),
    dk = _(`runtime.warning`),
    fk = _(`runtime.error`),
    X = S({
      eventId: H_,
      provider: ny,
      providerInstanceId: g(q),
      threadId: V,
      createdAt: B,
      turnId: g(W_),
      itemId: g(ev),
      requestId: g(nv),
      providerRefs: g(oO),
      raw: g(iO),
    }),
    pk = S({ message: g(Y), resume: g(v) }),
    mk = S({ config: rO }),
    hk = S({ state: sO, reason: g(Y), detail: g(v) }),
    gk = S({ reason: g(Y), recoverable: g(x), exitKind: g(pO) }),
    _k = S({ providerThreadId: g(Y) }),
    vk = S({ state: cO, beforeTokens: g(R), afterTokens: g(R), detail: g(v) }),
    yk = S({ name: g(Y), metadata: g(rO) }),
    bk = S({
      usedTokens: R,
      totalProcessedTokens: g(R),
      maxTokens: g(z),
      inputTokens: g(R),
      cachedInputTokens: g(R),
      outputTokens: g(R),
      reasoningOutputTokens: g(R),
      lastUsedTokens: g(R),
      lastInputTokens: g(R),
      lastCachedInputTokens: g(R),
      lastOutputTokens: g(R),
      lastReasoningOutputTokens: g(R),
      toolUses: g(R),
      durationMs: g(R),
      compactsAutomatically: g(x),
      autoCompactThreshold: g(z),
      cost: g(S({ amount: b.check(Pg()), currency: Y.check(j(32)) })),
    }),
    xk = S({ usage: bk }),
    Sk = S({ realtimeSessionId: g(Y) }),
    Ck = S({ item: v }),
    wk = S({ audio: v }),
    Tk = S({ message: Y }),
    Ek = S({ reason: g(Y) }),
    Dk = S({ model: g(Y), effort: g(Y) }),
    Ok = {
      usageScope: _(`main_agent`),
      cachedInputTokens: g(R),
      cacheCreationTokens: g(R),
      reasoningTokens: g(R),
      hasSubagents: x,
    },
    kk = T([
      S({ ...Ok, usageStatus: _(`complete`), inputTokens: R, outputTokens: R }),
      S({
        ...Ok,
        usageStatus: E([`partial`, `unavailable`]),
        inputTokens: g(R),
        outputTokens: g(R),
      }),
    ]),
    Ak = S({
      state: lO,
      stopReason: g(D(Y)),
      usage: g(v),
      modelUsage: g(rO),
      totalCostUsd: g(b),
      errorMessage: g(Y),
      tokenUsage: g(kk),
    }),
    jk = S({ reason: Y, tokenUsage: g(kk) }),
    Mk = S({ step: Y, status: uO }),
    Nk = S({ explanation: g(D(Y)), plan: w(Mk) }),
    Pk = S({ delta: y }),
    Fk = S({ planMarkdown: Y }),
    Ik = S({ unifiedDiff: y }),
    Lk = E([`browser`, `computer`]),
    Rk = T([
      Dg(`app-id`, { appId: Y.check(j(512), Mg(/^[A-Za-z0-9._-]+$/u)) }),
      Dg(`display-name`, { displayName: Y.check(j(160)) }),
    ]),
    zk = T([
      Dg(`website`, {
        pageUrl: Y.check(j(4096)),
        faviconUrl: g(Y.check(j(4096))),
        faviconUrlDark: g(Y.check(j(4096))),
      }),
      Dg(`native-app`, { app: Rk }),
      Dg(`themed-logo`, { logoUrl: Y.check(j(4096)), logoUrlDark: g(Y.check(j(4096))) }),
    ]),
    Bk = S({
      key: Y.check(j(512)),
      name: Y.check(j(160)),
      kind: E([`browser`, `computer`, `integration`]),
      icon: g(zk),
    }),
    Vk = S({
      itemType: gO,
      status: g(dO),
      title: g(Y),
      detail: g(Y),
      toolSurface: g(Lk),
      toolIcon: g(zk),
      toolSource: g(Bk),
      data: g(v),
      agentId: g(Y),
      parentToolUseId: g(Y),
    }),
    Hk = S({ streamKind: fO, delta: y, contentIndex: g(A), summaryIndex: g(A) }),
    Uk = S({ requestType: _O, detail: g(Y), appName: g(Y), options: g(w(KE)), args: g(v) }),
    Wk = S({ requestType: _O, decision: g(Y), resolution: g(v) }),
    Gk = S({
      questions: w(
        S({
          id: Y,
          header: Y,
          question: Y,
          options: w(S({ label: Y, description: y, value: g(y) })),
          allowCustomAnswer: g(x),
          initialAnswer: g(y),
          multiSelect: g(x).pipe(Cg(m(!1))),
        }),
      ),
      responseMode: g(_(`message`)),
    }),
    Kk = S({ answers: rO }),
    qk = S({
      totalTokens: R,
      inputTokens: g(R),
      cachedInputTokens: g(R),
      outputTokens: g(R),
      reasoningOutputTokens: g(R),
      toolUses: g(R),
      durationMs: g(R),
    }),
    Jk = S({ index: R, title: Y }),
    Yk = S({ runId: g(Y), scriptPath: g(Y), transcriptDir: g(Y), sessionUrl: g(Y) }),
    Xk = {
      taskType: g(Y),
      agentKind: g(E([`agent`, `background`])),
      agentId: g(Y),
      title: g(Y),
      role: g(Y),
      model: g(Y),
      effort: g(Y),
      toolUseId: g(Y),
      parentAgentId: g(Y),
      workflowName: g(Y),
      agentIndex: g(R),
      phaseIndex: g(R),
      phaseTitle: g(Y),
      phases: g(w(Jk)),
      attempt: g(R),
      runHandles: g(Yk),
      outputFile: g(Y),
      agentPath: g(Y),
      timelineBypass: g(x),
    };
  S(Xk);
  var Zk = S({ taskId: rv, description: g(Y), ...Xk }),
    Qk = E([
      `pending`,
      `running`,
      `waiting`,
      `idle`,
      `completed`,
      `failed`,
      `cancelled`,
      `interrupted`,
    ]),
    $k = S({
      taskId: rv,
      description: Y,
      summary: g(Y),
      usage: g(v),
      typedUsage: g(qk),
      lastToolName: g(Y),
      status: g(Qk),
      error: g(Y),
      ...Xk,
    }),
    eA = S({
      taskId: rv,
      status: g(Qk),
      description: g(Y),
      error: g(Y),
      endedAt: g(B),
      isBackgrounded: g(x),
      ...Xk,
    }),
    tA = S({
      taskId: rv,
      status: E([`completed`, `failed`, `stopped`]),
      summary: g(Y),
      usage: g(v),
      typedUsage: g(qk),
      ...Xk,
    }),
    nA = S({ hookId: Y, hookName: Y, hookEvent: Y }),
    rA = S({ hookId: Y, output: g(y), stdout: g(y), stderr: g(y) }),
    iA = S({
      hookId: Y,
      outcome: E([`success`, `error`, `cancelled`]),
      output: g(y),
      stdout: g(y),
      stderr: g(y),
      exitCode: g(A),
    }),
    aA = S({
      toolUseId: g(Y),
      toolName: g(Y),
      summary: g(Y),
      elapsedSeconds: g(b),
      taskId: g(rv),
      parentToolUseId: g(Y),
    }),
    oA = S({ summary: Y, precedingToolUseIds: g(w(Y)) }),
    sA = S({ isAuthenticating: g(x), output: g(w(y)), error: g(Y) }),
    cA = S({ account: v }),
    lA = S({ limits: XD }),
    uA = S({ status: v }),
    dA = S({ success: x, name: g(Y), error: g(Y) }),
    fA = S({ fromModel: Y, toModel: Y, reason: Y }),
    pA = S({ summary: Y, details: g(Y), path: g(Y), range: g(v) }),
    mA = S({ summary: Y, details: g(Y) }),
    hA = S({ files: w(S({ filename: Y, fileId: Y })), failed: g(w(S({ filename: Y, error: Y }))) }),
    gA = S({ toolName: Y, toolUseId: g(Y), reason: g(Y), agentId: g(Y) }),
    _A = S({ message: Y, detail: g(v) }),
    vA = S({ message: Y, code: g(Y), class: g(mO), detail: g(v) });
  T([
    S({ ...X.fields, type: vO, payload: pk }),
    S({ ...X.fields, type: yO, payload: mk }),
    S({ ...X.fields, type: bO, payload: hk }),
    S({ ...X.fields, type: xO, payload: gk }),
    S({ ...X.fields, type: SO, payload: _k }),
    S({ ...X.fields, type: CO, payload: vk }),
    S({ ...X.fields, type: wO, payload: yk }),
    S({ ...X.fields, type: TO, payload: xk }),
    S({ ...X.fields, type: EO, payload: Sk }),
    S({ ...X.fields, type: DO, payload: Ck }),
    S({ ...X.fields, type: OO, payload: wk }),
    S({ ...X.fields, type: kO, payload: Tk }),
    S({ ...X.fields, type: AO, payload: Ek }),
    S({ ...X.fields, type: jO, payload: Dk }),
    S({ ...X.fields, type: MO, payload: Ak }),
    S({ ...X.fields, type: NO, payload: jk }),
    S({ ...X.fields, type: PO, payload: Nk }),
    S({ ...X.fields, type: FO, payload: Pk }),
    S({ ...X.fields, type: IO, payload: Fk }),
    S({ ...X.fields, type: LO, payload: Ik }),
    S({ ...X.fields, type: RO, payload: Vk }),
    S({ ...X.fields, type: zO, payload: Vk }),
    S({ ...X.fields, type: BO, payload: Vk }),
    S({ ...X.fields, type: VO, payload: Hk }),
    S({ ...X.fields, type: HO, payload: Uk }),
    S({ ...X.fields, type: UO, payload: Wk }),
    S({ ...X.fields, type: WO, payload: Gk }),
    S({ ...X.fields, type: GO, payload: Kk }),
    S({ ...X.fields, type: KO, payload: Zk }),
    S({ ...X.fields, type: qO, payload: $k }),
    S({ ...X.fields, type: JO, payload: eA }),
    S({ ...X.fields, type: YO, payload: tA }),
    S({ ...X.fields, type: XO, payload: nA }),
    S({ ...X.fields, type: ZO, payload: rA }),
    S({ ...X.fields, type: QO, payload: iA }),
    S({ ...X.fields, type: $O, payload: aA }),
    S({ ...X.fields, type: ek, payload: oA }),
    S({ ...X.fields, type: tk, payload: sA }),
    S({ ...X.fields, type: nk, payload: cA }),
    S({ ...X.fields, type: rk, payload: lA }),
    S({ ...X.fields, type: ik, payload: uA }),
    S({ ...X.fields, type: ak, payload: dA }),
    S({ ...X.fields, type: ok, payload: fA }),
    S({ ...X.fields, type: sk, payload: pA }),
    S({ ...X.fields, type: ck, payload: mA }),
    S({ ...X.fields, type: lk, payload: hA }),
    S({ ...X.fields, type: uk, payload: gA }),
    S({ ...X.fields, type: dk, payload: _A }),
    S({ ...X.fields, type: fk, payload: vA }),
  ]);
  var yA = E([`user`, `agent`, `system`]),
    bA = E([`web`, `mobile`, `mcp`, `provider`, `server`]),
    xA = E([`native`, `v1_import`]),
    SA = { createdBy: yA, creationSource: bA },
    CA = E([`strong`, `weak`, `none`]),
    wA = S({ driver: ny, nativeId: D(L), strength: CA, fingerprint: g(L), ordinal: g(R) }),
    TA = S({
      parentThreadId: D(V),
      relationshipToParent: D(E([`fork`, `subagent`])),
      rootThreadId: V,
    }),
    EA = E([`fork`, `provider_handoff`, `merge_back`, `subagent_spawn`, `subagent_result`]),
    DA = S({
      threadId: V,
      runId: g(G),
      checkpointId: g(cv),
      turnItemId: g(tv),
      providerThreadRef: g(wA),
      providerTurnRef: g(wA),
    }),
    OA = T([
      S({ type: _(`latest_stable`) }),
      S({ type: _(`run`), runId: G }),
      S({ type: _(`checkpoint`), checkpointId: cv }),
    ]),
    kA = T([
      S({ strategy: _(`native_fork`), providerThreadRef: wA }),
      S({ strategy: _(`portable_context`), contextHandoffId: uv }),
      S({ strategy: _(`delta_context`), contextHandoffId: uv }),
      S({ strategy: _(`fork_delta_context`), contextHandoffId: uv }),
      S({ strategy: _(`checkpoint_context`), contextHandoffId: uv }),
    ]),
    AA = S({
      id: dv,
      type: EA,
      sourceThreadId: V,
      targetThreadId: V,
      sourcePoint: DA,
      basePoint: D(DA),
      sourceProviderInstanceId: D(q),
      targetProviderInstanceId: D(q),
      targetRunId: D(G),
      status: E([
        `pending`,
        `resolved_native`,
        `resolved_portable`,
        `failed`,
        `consumed`,
        `superseded`,
      ]),
      resolution: D(kA),
      createdBy: yA,
      error: D(y),
      createdAt: N,
      updatedAt: N,
      consumedAt: D(N),
    }),
    jA = S({
      supportsMultipleProviderThreadsPerSession: x,
      supportsModelSwitchInSession: x,
      supportsProviderSwitchingViaHandoff: x,
      supportsRuntimeModeSwitchInSession: x,
      pendingRequestsSurviveRestart: x,
    }),
    MA = S({
      canCreateEmptyThread: x,
      canReadThreadSnapshot: x,
      canRollbackThread: x,
      canForkThread: x,
      canForkFromTurn: x,
      canForkFromSubagentThread: x,
      exposesNativeThreadId: x,
    }),
    NA = S({
      exposesNativeTurnId: x,
      emitsTurnStarted: x,
      emitsTurnCompleted: x,
      supportsInterrupt: x,
      supportsActiveSteering: x,
      activeSteeringInterruptsTools: g(x),
      supportsSteeringByInterruptRestart: x,
      supportsQueuedMessages: x,
      terminalStatusQuality: E([`strong`, `weak`, `none`]),
    }),
    PA = S({
      streamsAssistantText: x,
      streamsReasoning: x,
      streamsToolOutput: x,
      streamsPlanText: x,
      emitsMessageCompleted: x,
    }),
    FA = S({
      exposesToolItemIds: x,
      emitsToolStarted: x,
      emitsToolCompleted: x,
      emitsToolOutput: x,
      supportsMcpTools: x,
      supportsDynamicToolCallbacks: x,
    }),
    IA = S({
      supportsCommandApproval: x,
      supportsFileReadApproval: x,
      supportsFileChangeApproval: x,
      supportsApplyPatchApproval: x,
      approvalsHaveNativeRequestIds: x,
      approvalCallbacksAreLiveOnly: x,
      approvalsCanOriginateFromSubagents: x,
    }),
    LA = S({
      emitsPlanUpdated: x,
      emitsTodoList: x,
      emitsProposedPlan: x,
      supportsStructuredQuestions: x,
      planDeltasHaveItemIds: x,
    }),
    RA = S({
      supportsSubagents: x,
      exposesSubagentThreadIds: x,
      emitsSubagentLifecycle: x,
      canWaitForSubagents: x,
      canCloseSubagents: x,
      canForkSubagentThread: x,
    }),
    zA = S({
      acceptsSystemContext: x,
      acceptsDeveloperContext: x,
      acceptsSyntheticUserContext: x,
      canGenerateSummaries: x,
      canConsumeHandoffSummaries: x,
      supportsDeltaHandoff: x,
      supportsFullThreadHandoff: x,
      maxRecommendedHandoffChars: D(z),
    }),
    BA = S({
      appCanCheckpointFilesystem: x,
      supportsNestedCheckpointScopes: x,
      providerCanRollbackConversation: x,
      providerRollbackReturnsSnapshot: x,
      providerCanReadConversationSnapshot: x,
    }),
    VA = S({ enforcement: E([`native`, `client-boundary`]) }),
    HA = S({
      sessions: jA,
      threads: MA,
      turns: NA,
      streaming: PA,
      tools: FA,
      approvals: IA,
      planning: LA,
      subagents: RA,
      context: zA,
      checkpointing: BA,
      identity: S({
        nativeThreadIds: CA,
        nativeTurnIds: CA,
        nativeItemIds: CA,
        nativeRequestIds: CA,
      }),
      runtimePolicy: VA.pipe(O(m({ enforcement: `client-boundary` }))),
    }),
    UA = S({ requestId: g(W), runId: G, resetAt: B, autoResume: x, snooze: g(x) }),
    WA = S({ runId: G, resetAt: B, autoResume: g(x), snooze: g(x) }).check(
      k(
        (e) =>
          e.autoResume !== void 0 ||
          e.snooze !== void 0 ||
          `A recovery update must include autoResume or snooze.`,
      ),
    ),
    GA = S({
      ...SA,
      id: V,
      projectId: H,
      title: L,
      providerInstanceId: q,
      modelSelection: bw,
      runtimeMode: VE,
      interactionMode: UE,
      branch: D(L),
      worktreePath: D(L),
      linkedPullRequest: g(D(jE)),
      pullRequests: g(w(RE)),
      branchPullRequest: g(D(jE)),
      activeProviderThreadId: D(Q_),
      historyOrigin: g(xA),
      lineage: TA,
      forkedFrom: D(
        T([
          S({ type: _(`run`), threadId: V, runId: G }),
          S({ type: _(`node`), nodeId: K }),
          S({ type: _(`provider_thread`), providerThreadId: Q_, providerTurnId: g($_) }),
        ]),
      ),
      createdAt: N,
      updatedAt: N,
      archivedAt: D(N),
      settledOverride: D(E([`settled`, `active`])).pipe(O(m(null))),
      settledAt: D(N).pipe(O(m(null))),
      unsettledAt: g(D(N)),
      snoozedUntil: g(D(N)),
      snoozedAt: g(D(N)),
      lastSnoozeWakeAt: g(D(N)),
      limitRecovery: g(D(UA)),
      pinnedAt: g(D(N)),
      autoSettleDisabledAt: g(D(N)),
      pinOrderKey: g(D(L)),
      activeOrderKey: g(D(L)),
      lastVisitedAt: D(N).pipe(O(m(null))),
      titleRegeneration: g(D(S({ requestId: W, startedAt: N }))),
      rollbackRequestId: g(W),
      rollbackFailure: g(D(S({ requestId: W, message: L }))),
      deletedAt: D(N),
    }),
    KA = E([
      `preparing`,
      `queued`,
      `starting`,
      `running`,
      `waiting`,
      `completed`,
      `interrupted`,
      `failed`,
      `cancelled`,
      `rolled_back`,
    ]),
    qA = S({
      state: E([`pending`, `claimed`, `acknowledged`, `delivered`, `disposed`]),
      observedByRunId: D(G),
    }),
    JA = S({ generation: z, messageId: U_, taskIds: w(K) }),
    YA = S({ disposition: E([`open`, `stopped`, `disposed`]), nextGeneration: z, delivery: D(JA) }),
    XA = S({ kind: E([`subagent`, `shell`, `monitor`, `task`]), label: L, id: g(L) }),
    ZA = S({ runId: G, restartCancelledBackgroundWork: w(XA) }),
    QA = T([
      S({ type: _(`root`), branch: g(L) }),
      S({ type: _(`existing_worktree`), worktreePath: L, branch: g(L) }),
      S({ type: _(`worktree`), baseRef: L, branch: g(L), startFromOrigin: g(x) }),
    ]),
    $A = S({
      id: G,
      threadId: V,
      ordinal: z,
      providerInstanceId: q,
      modelSelection: bw,
      providerThreadId: D(Q_),
      userMessageId: U_,
      rootNodeId: D(K),
      activeAttemptId: D(G_),
      status: KA,
      queuePosition: g(D(z)),
      queueHeld: g(x),
      requestedAt: N,
      startedAt: D(N),
      completedAt: D(N),
      checkpointId: D(cv),
      contextHandoffId: D(uv),
      restartContinuationOfRunId: g(G),
      delegatedTaskId: g(K),
      workStartedAt: g(N),
      restartCancelledBackgroundWork: g(w(XA)),
      sourcePlanRef: g(S({ threadId: V, planId: pv })),
      delegatedCompletion: g(YA),
      workspacePreparation: g(QA),
    }),
    ej = S({
      id: G_,
      nativeThreadId: g(y),
      runId: G,
      attemptOrdinal: z,
      rootNodeId: K,
      providerInstanceId: q,
      providerThreadId: Q_,
      providerTurnId: D($_),
      reason: E([`initial`, `steering_restart`, `retry`, `provider_recovery`]),
      status: E([
        `pending`,
        `running`,
        `completed`,
        `interrupted`,
        `failed`,
        `cancelled`,
        `superseded`,
      ]),
      startedAt: D(N),
      completedAt: D(N),
    }),
    tj = S({
      id: K,
      threadId: V,
      runId: D(G),
      parentNodeId: D(K),
      rootNodeId: K,
      kind: E([
        `root_turn`,
        `assistant_message`,
        `reasoning`,
        `plan`,
        `todo_list`,
        `tool_call`,
        `approval_request`,
        `user_input_request`,
        `subagent`,
        `hook`,
        `system`,
      ]),
      status: E([
        `idle`,
        `pending`,
        `running`,
        `waiting`,
        `completed`,
        `interrupted`,
        `failed`,
        `cancelled`,
        `rolled_back`,
      ]),
      countsForRun: x,
      providerThreadId: D(Q_),
      providerTurnId: D($_),
      nativeItemRef: D(wA),
      runtimeRequestId: D(nv),
      checkpointScopeId: D(lv),
      startedAt: D(N),
      completedAt: D(N),
    }),
    nj = S({ index: R, title: L }),
    rj = S({
      index: R,
      label: L,
      agentId: g(L),
      childThreadId: g(V),
      state: E([`queued`, `running`, `completed`, `failed`]),
      phaseIndex: g(R),
      phaseTitle: g(L),
      model: g(L),
      attempt: g(R),
      totalTokens: g(R),
      toolCalls: g(R),
      durationMs: g(R),
      queuedAt: g(R),
      startedAt: g(R),
      prompt: g(Qg),
      result: g(Qg),
    }),
    ij = S({ runId: g(L), transcriptDir: g(L), scriptPath: g(L) }),
    aj = S({
      name: g(L),
      runHandles: g(ij),
      phases: w(nj),
      agents: w(rj),
      totalTokens: g(R),
      toolCalls: g(R),
      durationMs: g(R),
    }),
    oj = S({
      id: K,
      threadId: V,
      runId: D(G),
      parentNodeId: K,
      origin: E([`provider_native`, `app_owned`]),
      createdBy: yA,
      driver: ny,
      providerInstanceId: q,
      providerThreadId: D(Q_),
      childThreadId: D(V),
      nativeTaskRef: D(wA),
      prompt: y,
      title: D(y),
      model: D(y),
      modelSelection: g(bw),
      completionWake: g(E([`always`, `settled_only`])),
      completionDelivery: g(qA),
      status: E([
        `idle`,
        `pending`,
        `running`,
        `waiting`,
        `completed`,
        `failed`,
        `cancelled`,
        `interrupted`,
      ]),
      progress: g(y),
      workflow: g(aj),
      result: D(y),
      startedAt: D(N),
      completedAt: D(N),
      updatedAt: N,
    }),
    sj = S({
      id: lv,
      threadId: V,
      runId: D(G),
      nodeId: K,
      parentScopeId: D(lv),
      providerThreadId: D(Q_),
      kind: E([`root_run`, `subagent`, `tool`, `provider_thread`, `manual`]),
      ordinalWithinParent: R,
      advancesAppRunCount: x,
      cwd: L,
      createdAt: N,
    }),
    cj = S({
      id: Z_,
      driver: ny,
      providerInstanceId: q,
      status: E([`starting`, `ready`, `running`, `waiting`, `stopped`, `error`]),
      cwd: L,
      model: D(L),
      capabilities: HA,
      createdAt: N,
      updatedAt: N,
      lastError: D(y),
    }),
    lj = S({ providerSessionId: Z_, detachedAt: N, reason: g(y) });
  function uj(e) {
    let t = Zp(e.ast),
      n = Zd(t) ? t.propertySignatures.find((e) => e.name === `kind`)?.type : void 0;
    if (n !== void 0 && Jd(n) && typeof n.literal == `string`) return n.literal;
    throw Error("Each member of a kind union needs a literal string `kind`.");
  }
  function dj(e, t, n) {
    let r = new Set(e.map(uj)),
      i = t(g(y.check(k((e) => !r.has(e) || `A known kind must decode in full.`)))).pipe(
        xg(T(e), { decode: ad(n), encode: td(() => `Unknown kinds are decode-only.`) }),
      );
    return T([...e, i]);
  }
  var fj = { taskId: L, description: g(L) },
    pj = dj(
      [
        S({ ...fj, kind: _(`subagent`), childThreadId: g(V) }),
        S({ ...fj, kind: _(`command`) }),
        S({ ...fj, kind: _(`monitor`) }),
        S({ ...fj, kind: _(`background_task`) }),
      ],
      (e) => S({ ...fj, kind: e }),
      ({ taskId: e, description: t }) => ({
        taskId: e,
        ...(t === void 0 ? {} : { description: t }),
        kind: `background_task`,
      }),
    ),
    mj = S({
      modelSelection: g(bw),
      title: g(D(L)),
      updatedAt: g(D(L)),
      itemIdentityVersion: g(_(2)),
    }),
    hj = S({
      objective: L,
      status: E([`active`, `paused`, `blocked`, `usage_limited`, `budget_limited`, `complete`]),
      tokensUsed: g(R),
      tokenBudget: g(D(R)),
      timeUsedSeconds: g(R),
      checks: g(R),
      lastCheck: g(y),
    }),
    gj = S({
      id: Q_,
      driver: ny,
      providerInstanceId: q,
      providerSessionId: D(Z_),
      appThreadId: D(V),
      ownerNodeId: D(K),
      nativeThreadRef: D(wA),
      nativeConversationHeadRef: D(wA),
      status: E([`not_loaded`, `idle`, `active`, `archived`, `closed`, `error`]),
      firstRunOrdinal: D(z),
      lastRunOrdinal: D(z),
      handoffIds: w(uv),
      forkedFrom: D(S({ providerThreadId: Q_, providerTurnId: g($_), checkpointId: g(cv) })),
      pendingBackgroundTasks: g(w(pj)).pipe(O(m([]))),
      contextUsage: g(D(bk)).pipe(O(m(null))),
      nativeMetadata: g(D(mj)).pipe(O(m(null))),
      goal: g(D(hj)).pipe(O(m(null))),
      createdAt: N,
      updatedAt: N,
    }),
    _j = S({
      role: E([`user`, `assistant`]),
      text: y,
      runStatus: g(y),
      threadId: V,
      runId: D(G),
      itemId: tv,
      providerThreadId: D(Q_),
      status: y,
      kind: y,
    }),
    vj = S({
      id: uv,
      transferId: g(D(dv)),
      threadId: V,
      targetRunId: G,
      fromProviderThreadIds: w(Q_),
      toProviderThreadId: Q_,
      coveredRunOrdinals: S({ from: z, to: z }),
      strategy: E([
        `delta_since_target_last_seen`,
        `fork_delta_summary`,
        `full_thread_summary`,
        `checkpoint_summary`,
        `manual_context`,
      ]),
      status: E([`pending`, `ready`, `failed`, `superseded`]),
      summaryMessageId: D(U_),
      summaryText: y,
      history: g(S({ messages: w(_j), coverage: y, omittedItems: R, omittedItemIds: g(w(tv)) })),
      delivery: g(
        S({
          nativeThreadId: y,
          status: E([`pending`, `injected`, `inline`]),
          itemIds: w(tv),
          omittedItemIds: g(w(tv)),
        }),
      ),
      detailInTurnItem: g(_(!0)),
      createdByProviderInstanceId: D(q),
      createdAt: N,
      updatedAt: N,
    }),
    yj = S({
      usedTokens: R,
      maxTokens: g(D(R)),
      inputTokens: g(R),
      cachedInputTokens: g(R),
      outputTokens: g(R),
      reasoningOutputTokens: g(R),
      promptCacheTtlMs: g(z),
      updatedAt: y,
    }),
    bj = S({
      id: $_,
      providerThreadId: Q_,
      nodeId: K,
      runAttemptId: D(G_),
      nativeTurnRef: D(wA),
      ordinal: z,
      status: E([`pending`, `running`, `completed`, `interrupted`, `failed`, `cancelled`]),
      startedAt: D(N),
      completedAt: D(N),
      tokenUsage: g(yj),
      turnTokenUsage: g(kk),
    }),
    xj = S({
      id: nv,
      nodeId: K,
      providerTurnId: D($_),
      nativeRequestRef: D(wA),
      kind: T([WE, E([`dynamic_tool_call`, `user_input`, `auth_refresh`])]),
      status: E([`pending`, `resolved`, `expired`, `cancelled`]),
      responseCapability: T([
        S({ type: _(`live`), providerSessionId: Z_ }),
        S({ type: _(`message`) }),
        S({ type: _(`not_resumable`), reason: y }),
      ]),
      createdAt: N,
      resolvedAt: D(N),
      decision: g(GE),
      answers: g(qE),
    }),
    Sj = S({ kind: _(`subagent`), childThreadId: g(V) }),
    Cj = S({ kind: _(`command`) }),
    wj = S({
      source: dj(
        [
          S({ kind: _(`delegated_task`), taskIds: w(K), childThreadId: g(V) }),
          S({ kind: _(`background_task`), work: _(`subagent`), childThreadId: g(V) }).pipe(
            xg($h(Sj), {
              decode: ad(({ childThreadId: e }) =>
                e === void 0 ? { kind: `subagent` } : { kind: `subagent`, childThreadId: e },
              ),
              encode: ad(({ childThreadId: e }) => ({
                kind: `background_task`,
                work: `subagent`,
                ...(e === void 0 ? {} : { childThreadId: e }),
              })),
            }),
          ),
          S({ kind: _(`background_command`).transform(`command`) }),
          Sj,
          Cj,
          S({ kind: _(`monitor`) }),
          S({ kind: _(`background_task`) }),
          S({ kind: _(`system`) }),
        ],
        (e) => S({ kind: e }),
        () => ({ kind: `background_task` }),
      ),
      outcome: E([`completed`, `failed`, `cancelled`, `updated`, `unknown`]),
      summary: L,
      detail: g(y),
    }),
    Tj = S({
      notification: g(wj),
      ...SA,
      scheduledTaskId: g(iv),
      senderThreadId: g(V),
      id: U_,
      threadId: V,
      runId: D(G),
      nodeId: D(K),
      role: E([`user`, `assistant`, `system`]),
      text: y,
      context: g(Xv),
      attachments: w(ow),
      streaming: x,
      createdAt: N,
      updatedAt: N,
      delegatedCompletion: g(S({ parentRunId: G, generation: z, taskIds: w(K) })),
    }),
    Ej = S({
      id: L,
      text: L,
      status: E([`pending`, `running`, `completed`]),
      durationAnchorAt: g(B),
      durationMs: g(R),
    }),
    Dj = S({
      id: L,
      header: L,
      question: L,
      options: w(S({ label: L, description: L, value: g(y) })),
      multiSelect: g(x),
      allowCustomAnswer: g(x),
      initialAnswer: g(y),
      required: g(x),
    }),
    Oj = {
      id: pv,
      threadId: V,
      runId: D(G),
      nodeId: K,
      status: E([`draft`, `active`, `completed`, `superseded`]),
      detailInTurnItem: g(_(!0)),
    },
    kj = T([
      S({ ...Oj, kind: _(`proposed_plan`), markdown: y }),
      S({ ...Oj, kind: _(`todo_list`), steps: w(Ej), explanation: g(y) }),
    ]),
    Aj = S({ path: L, kind: L, additions: R, deletions: R }),
    jj = S({
      id: cv,
      threadId: V,
      scopeId: lv,
      runId: D(G),
      nodeId: K,
      parentCheckpointId: D(cv),
      ordinalWithinScope: R,
      appRunOrdinal: D(z),
      ref: sv,
      status: E([`ready`, `missing`, `error`, `stale`]),
      files: w(Aj),
      capturedAt: N,
    }),
    Mj = S({ scopeId: lv, checkpointId: cv, requestedAt: N });
  F()(`OrchestrationV2CheckpointUnavailableError`, { threadId: V, target: y });
  var Nj = E([
      `idle`,
      `pending`,
      `running`,
      `waiting`,
      `completed`,
      `failed`,
      `cancelled`,
      `interrupted`,
    ]),
    Pj = S({ operation: L, path: L, oldPath: g(L), fileType: g(L), mimeType: g(L) }),
    Fj = E([
      `usage_limit`,
      `provider_error`,
      `transport_error`,
      `permission_error`,
      `validation_error`,
      `unknown`,
    ]),
    Ij = S({
      class: Fj,
      message: L.check(j(4096)),
      code: D(L.check(j(128))),
      retryable: D(x),
      resetAt: g(D(B)),
    }),
    Lj = S({ attempt: z, maxAttempts: D(z), retryDelayMs: D(R) });
  E([`reusable`, `broken`]);
  var Rj = E([`turn_start`, `queued_turn`, `steer`, `promoted_queued_to_steer`]),
    zj = {
      toolNonExecutionKind: g(y),
      toolSurface: g(Lk),
      toolIcon: g(zk),
      toolSource: g(Bk),
      id: tv,
      threadId: V,
      runId: D(G),
      nodeId: D(K),
      providerThreadId: D(Q_),
      providerTurnId: D($_),
      nativeItemRef: D(wA),
      parentItemId: D(tv),
      ordinal: R,
      status: Nj,
      title: D(y),
      startedAt: D(N),
      completedAt: D(N),
      updatedAt: N,
    },
    Bj = S({ fileName: L, line: g(z), column: g(z), preview: g(y) }),
    Vj = S({ title: g(y), url: g(L), snippet: g(y) }),
    Hj = E([`pending`, `saved`, `declined`, `cancelled`]),
    Uj = { type: _(`secret_request`), label: L, reason: y, placeholder: g(y), secretStatus: Hj },
    Wj = T([
      S({ ...zj, type: _(`notification`), ...wj.fields }),
      S({
        ...zj,
        ...SA,
        type: _(`user_message`),
        messageId: U_,
        scheduledTaskId: g(iv),
        senderThreadId: g(V),
        inputIntent: Rj,
        text: y,
        context: g(Xv),
        attachments: w(ow),
      }),
      S({
        ...zj,
        type: _(`assistant_message`),
        messageId: U_,
        text: y,
        attachments: g(w(ow)),
        streaming: x,
      }),
      S({ ...zj, type: _(`reasoning`), text: y, streaming: x }),
      S({ ...zj, type: _(`proposed_plan`), planId: pv, markdown: y, streaming: x }),
      S({ ...zj, type: _(`todo_list`), planId: pv, steps: w(Ej), explanation: g(y) }),
      S({
        ...zj,
        type: _(`user_input_request`),
        requestId: nv,
        questions: w(Dj),
        questionAnswer: g(YE),
        responseMode: g(_(`message`)),
      }),
      S({
        ...zj,
        type: _(`file_change`),
        fileName: L,
        additions: g(R),
        deletions: g(R),
        diffStr: g(y),
        oldStr: g(y),
        newStr: g(y),
        changes: g(w(Pj)),
      }),
      S({
        ...zj,
        type: _(`command_execution`),
        input: y,
        output: g(y),
        outputOmitted: g(x),
        outputIndicatesFailure: g(x),
        exitCode: g(A),
      }),
      S({ ...zj, type: _(`file_search`), pattern: g(y), results: g(w(Bj)) }),
      S({ ...zj, type: _(`web_search`), patterns: g(w(y)), results: g(w(Vj)) }),
      S({
        ...zj,
        type: _(`approval_request`),
        requestId: nv,
        requestKind: WE,
        prompt: g(y),
        appName: g(y),
        options: g(w(KE)),
      }),
      S({ ...zj, type: _(`checkpoint`), checkpointId: cv, scopeId: lv, files: w(Aj) }),
      S({ ...zj, type: _(`run_interrupt_request`), message: y }),
      S({ ...zj, type: _(`run_interrupt_result`), message: y }),
      S({ ...zj, type: _(`system_notice`), message: y }),
      S({ ...zj, type: _(`error`), failure: Ij, retry: g(Lj) }),
      S({
        ...zj,
        type: _(`compaction`),
        driver: D(ny),
        summary: g(y),
        beforeTokenCount: g(R),
        afterTokenCount: g(R),
      }),
      S({
        ...zj,
        type: _(`handoff`),
        contextHandoffId: uv,
        fromProviderThreadIds: w(Q_),
        toProviderThreadId: Q_,
        fromProviderInstanceIds: w(q),
        toProviderInstanceId: q,
        fromModelSelections: g(w(bw)),
        toModel: g(y),
        strategy: E([
          `delta_since_target_last_seen`,
          `fork_delta_summary`,
          `full_thread_summary`,
          `checkpoint_summary`,
          `manual_context`,
        ]),
        summary: g(y),
      }),
      S({
        ...zj,
        type: _(`fork`),
        source: T([
          S({ type: _(`run`), threadId: V, runId: G }),
          S({ type: _(`node`), nodeId: K }),
          S({ type: _(`provider_thread`), providerThreadId: Q_, providerTurnId: g($_) }),
        ]),
        targetThreadId: V,
        providerThreadId: g(Q_),
      }),
      S({
        ...zj,
        type: _(`thread_created`),
        targetThreadId: V,
        targetRunId: D(G),
        targetProviderInstanceId: q,
        targetModel: L,
      }),
      S({ ...zj, ...Uj }),
      S({
        ...zj,
        type: _(`subagent`),
        subagentId: K,
        origin: E([`provider_native`, `app_owned`]),
        driver: ny,
        providerInstanceId: q,
        childThreadId: D(V),
        prompt: y,
        progress: g(y),
        result: D(y),
      }),
      S({
        ...zj,
        type: _(`dynamic_tool`),
        toolName: D(L),
        viewedImagePath: g(L),
        input: v,
        output: g(v),
        outputOmitted: g(x),
      }),
    ]),
    Gj = L_(Wj.members, `type`),
    Kj = (e) => z_(e.members, `type`),
    qj = (e, t) =>
      w(t).pipe(
        xg(w($h(e)), Ed({ decode: (e) => e.filter((e) => !R_(e.item)), encode: (e) => e })),
      ),
    Jj = S({
      position: R,
      visibility: E([`local`, `inherited`, `synthetic`]),
      sourceThreadId: V,
      sourceItemId: tv,
      item: Wj,
    }),
    Yj = S({
      id: fv,
      driver: ny,
      providerInstanceId: q,
      providerSessionId: Z_,
      sequence: z,
      direction: E([`incoming`, `outgoing`]),
      messageKind: E([`request`, `response`, `notification`, `error`]),
      method: D(L),
      jsonRpcId: D(T([y, b])),
      payload: v,
      observedAt: N,
    }),
    Xj = S({
      id: H_,
      threadId: V,
      runId: g(G),
      nodeId: g(K),
      driver: g(ny),
      providerInstanceId: g(q),
      rawEventId: g(fv),
      occurredAt: N,
    }),
    Zj = T([
      S({ ...Xj.fields, type: _(`thread.created`), payload: GA }),
      S({
        ...Xj.fields,
        type: E([
          `thread.archived`,
          `thread.unarchived`,
          `thread.deleted`,
          `thread.settled`,
          `thread.unsettled`,
          `thread.snoozed`,
          `thread.unsnoozed`,
          `thread.pinned`,
          `thread.auto-settle-set`,
          `thread.unpinned`,
          `thread.pin-reordered`,
          `thread.active-reordered`,
          `thread.visited`,
          `thread.marked-unread`,
          `thread.metadata-updated`,
          `thread.pull-request-synced`,
          `thread.runtime-mode-updated`,
          `thread.interaction-mode-updated`,
          `thread.model-selection-updated`,
          `thread.provider-switched`,
        ]),
        payload: GA,
      }),
      S({ ...Xj.fields, type: _(`run.created`), payload: $A }),
      S({ ...Xj.fields, type: _(`run.updated`), payload: $A }),
      S({ ...Xj.fields, type: _(`run.background-work-cancelled`), payload: ZA }),
      S({ ...Xj.fields, type: _(`run-attempt.created`), payload: ej }),
      S({ ...Xj.fields, type: _(`run-attempt.updated`), payload: ej }),
      S({ ...Xj.fields, type: _(`node.updated`), payload: tj }),
      S({ ...Xj.fields, type: _(`subagent.updated`), payload: oj }),
      S({
        ...Xj.fields,
        type: E([`provider-session.attached`, `provider-session.updated`]),
        payload: cj,
      }),
      S({ ...Xj.fields, type: _(`provider-session.detached`), payload: lj }),
      S({ ...Xj.fields, type: _(`provider-thread.updated`), payload: gj }),
      S({ ...Xj.fields, type: _(`provider-turn.updated`), payload: bj }),
      S({ ...Xj.fields, type: _(`runtime-request.updated`), payload: xj }),
      S({ ...Xj.fields, type: _(`message.updated`), payload: Tj }),
      S({ ...Xj.fields, type: _(`turn-item.updated`), payload: Wj }),
      S({ ...Xj.fields, type: _(`plan.updated`), payload: kj }),
      S({ ...Xj.fields, type: _(`checkpoint-scope.created`), payload: sj }),
      S({ ...Xj.fields, type: _(`checkpoint.captured`), payload: jj }),
      S({ ...Xj.fields, type: _(`checkpoint.rollback-requested`), payload: Mj }),
      S({ ...Xj.fields, type: _(`context-handoff.updated`), payload: vj }),
      S({ ...Xj.fields, type: _(`context-transfer.created`), payload: AA }),
      S({ ...Xj.fields, type: _(`context-transfer.updated`), payload: AA }),
    ]),
    Qj = S({
      thread: GA,
      runs: w($A),
      attempts: w(ej),
      nodes: w(tj),
      subagents: w(oj),
      providerSessions: w(cj),
      providerThreads: w(gj),
      providerTurns: w(bj),
      runtimeRequests: w(xj),
      messages: w(Tj),
      plans: w(kj),
      turnItems: Kj(Wj),
      checkpointScopes: w(sj),
      checkpoints: w(jj),
      contextHandoffs: w(vj),
      contextTransfers: w(AA),
      visibleTurnItems: qj(
        Jj,
        Jj.mapFields((e) => ({ ...e, item: I_(Wj.members, `type`) })),
      ),
      updatedAt: N,
    }),
    $j = T([_(`idle`), KA]),
    eM = S({ id: nv, kind: xj.fields.kind, createdAt: N }),
    tM = S({ id: U_, role: Tj.fields.role, text: y, updatedAt: N }),
    nM = S({
      ...SA,
      id: V,
      projectId: H,
      title: y,
      providerInstanceId: q,
      modelSelection: bw,
      runtimeMode: VE,
      interactionMode: UE,
      branch: D(L),
      worktreePath: D(L),
      linkedPullRequest: g(D(jE)),
      pullRequests: g(w(RE)),
      branchPullRequest: g(D(jE)),
      lineage: TA,
      forkedFrom: D(GA.fields.forkedFrom),
      activeProviderThreadId: D(Q_),
      historyOrigin: g(xA),
      latestRunId: D(G),
      latestRunRequestedAt: g(D(N)),
      latestRunStartedAt: g(D(N)),
      latestRunCompletedAt: g(D(N)),
      activeRunId: D(G),
      activityRunStartedAt: g(D(N)),
      activityRunStatus: g(D(E([`preparing`, `starting`, `running`, `waiting`]))),
      status: $j,
      lastError: g(D(y)),
      lastErrorClass: g(D(Fj)),
      usageLimitResetAt: g(D(B)),
      pendingRuntimeRequest: D(eM),
      latestVisibleMessage: D(tM),
      latestUserMessageAt: D(N),
      latestUserAuthoredMessageAt: g(D(N)),
      hasActionableProposedPlan: x,
      pendingBackgroundTasks: g(w(pj)).pipe(O(m([]))),
      providerInstanceHistory: g(w(q)).pipe(O(m([]))),
      goal: g(D(hj)),
      itemCount: R,
      visibleItemCount: R,
      createdAt: N,
      updatedAt: N,
      archivedAt: D(N),
      settledOverride: D(E([`settled`, `active`])),
      settledAt: D(N),
      unsettledAt: g(D(N)),
      snoozedUntil: g(D(N)),
      snoozedAt: g(D(N)),
      limitRecovery: g(D(UA)),
      pinnedAt: g(D(N)),
      autoSettleDisabledAt: g(D(N)),
      pinOrderKey: g(D(L)),
      activeOrderKey: g(D(L)),
      lastVisitedAt: g(D(N)),
      titleRegeneration: g(D(S({ requestId: W, startedAt: N }))),
      deletedAt: D(N),
    }),
    rM = S({
      ...S({ schemaVersion: z, snapshotSequence: R, threads: w(nM), archivedThreads: w(nM) })
        .fields,
      projects: w(GD),
    }),
    iM = T([
      S({ kind: _(`synchronized`) }),
      S({ kind: _(`snapshot`), snapshot: rM, resolvedRepositoryIdentityRoots: h(w(y)) }),
      S({ kind: _(`project.updated`), sequence: R, project: GD }),
      S({ kind: _(`project.removed`), sequence: R, projectId: H }),
      S({ kind: _(`thread.updated`), sequence: R, location: E([`active`, `archive`]), thread: nM }),
      S({
        kind: _(`thread.removed`),
        sequence: R,
        location: E([`active`, `archive`]),
        threadId: V,
      }),
    ]);
  S({ sequence: R, commandId: D(W), event: Zj });
  var aM = GA.mapFields((e) => ({
      ...e,
      createdAt: P,
      updatedAt: P,
      archivedAt: D(P),
      settledAt: D(P).pipe(O(m(null))),
      unsettledAt: g(D(P)),
      snoozedUntil: g(D(P)),
      snoozedAt: g(D(P)),
      lastSnoozeWakeAt: g(D(P)),
      pinnedAt: g(D(P)),
      autoSettleDisabledAt: g(D(P)),
      lastVisitedAt: D(P).pipe(O(m(null))),
      titleRegeneration: g(D(S({ requestId: W, startedAt: P }))),
      deletedAt: D(P),
    })),
    oM = $A.mapFields((e) => ({
      ...e,
      requestedAt: P,
      startedAt: D(P),
      completedAt: D(P),
      workStartedAt: g(P),
    })),
    sM = ej.mapFields((e) => ({ ...e, startedAt: D(P), completedAt: D(P) })),
    cM = tj.mapFields((e) => ({ ...e, startedAt: D(P), completedAt: D(P) })),
    lM = oj.mapFields((e) => ({ ...e, startedAt: D(P), completedAt: D(P), updatedAt: P })),
    uM = sj.mapFields((e) => ({ ...e, createdAt: P })),
    dM = cj.mapFields((e) => ({ ...e, createdAt: P, updatedAt: P })),
    fM = lj.mapFields((e) => ({ ...e, detachedAt: P })),
    pM = gj.mapFields((e) => ({ ...e, createdAt: P, updatedAt: P })),
    mM = vj.mapFields((e) => ({ ...e, createdAt: P, updatedAt: P })),
    hM = AA.mapFields((e) => ({ ...e, createdAt: P, updatedAt: P, consumedAt: D(P) })),
    gM = bj.mapFields((e) => ({ ...e, startedAt: D(P), completedAt: D(P) })),
    _M = xj.mapFields((e) => ({ ...e, createdAt: P, resolvedAt: D(P) })),
    vM = Tj.mapFields((e) => ({ ...e, createdAt: P, updatedAt: P })),
    yM = jj.mapFields((e) => ({ ...e, capturedAt: P })),
    bM = Mj.mapFields((e) => ({ ...e, requestedAt: P })),
    xM = { ...zj, startedAt: D(P), completedAt: D(P), updatedAt: P },
    SM = T([
      S({ ...xM, type: _(`notification`), ...wj.fields }),
      S({
        ...xM,
        ...SA,
        type: _(`user_message`),
        messageId: U_,
        scheduledTaskId: g(iv),
        senderThreadId: g(V),
        inputIntent: Rj,
        text: y,
        context: g(Xv),
        attachments: w(ow),
      }),
      S({
        ...xM,
        type: _(`assistant_message`),
        messageId: U_,
        text: y,
        attachments: g(w(ow)),
        streaming: x,
      }),
      S({ ...xM, type: _(`reasoning`), text: y, streaming: x }),
      S({ ...xM, type: _(`proposed_plan`), planId: pv, markdown: y, streaming: x }),
      S({ ...xM, type: _(`todo_list`), planId: pv, steps: w(Ej), explanation: g(y) }),
      S({
        ...xM,
        type: _(`user_input_request`),
        requestId: nv,
        questions: w(Dj),
        questionAnswer: g(YE),
        responseMode: g(_(`message`)),
      }),
      S({
        ...xM,
        type: _(`file_change`),
        fileName: L,
        additions: g(R),
        deletions: g(R),
        diffStr: g(y),
        oldStr: g(y),
        newStr: g(y),
        changes: g(w(Pj)),
      }),
      S({
        ...xM,
        type: _(`command_execution`),
        input: y,
        output: g(y),
        outputOmitted: g(x),
        outputIndicatesFailure: g(x),
        exitCode: g(A),
      }),
      S({ ...xM, type: _(`file_search`), pattern: g(y), results: g(w(Bj)) }),
      S({ ...xM, type: _(`web_search`), patterns: g(w(y)), results: g(w(Vj)) }),
      S({
        ...xM,
        type: _(`approval_request`),
        requestId: nv,
        requestKind: WE,
        prompt: g(y),
        appName: g(y),
        options: g(w(KE)),
      }),
      S({ ...xM, type: _(`checkpoint`), checkpointId: cv, scopeId: lv, files: w(Aj) }),
      S({ ...xM, type: _(`run_interrupt_request`), message: y }),
      S({ ...xM, type: _(`run_interrupt_result`), message: y }),
      S({ ...xM, type: _(`system_notice`), message: y }),
      S({ ...xM, type: _(`error`), failure: Ij, retry: g(Lj) }),
      S({
        ...xM,
        type: _(`compaction`),
        driver: D(ny),
        summary: g(y),
        beforeTokenCount: g(R),
        afterTokenCount: g(R),
      }),
      S({
        ...xM,
        type: _(`handoff`),
        contextHandoffId: uv,
        fromProviderThreadIds: w(Q_),
        toProviderThreadId: Q_,
        fromProviderInstanceIds: w(q),
        toProviderInstanceId: q,
        fromModelSelections: g(w(bw)),
        toModel: g(y),
        strategy: E([
          `delta_since_target_last_seen`,
          `fork_delta_summary`,
          `full_thread_summary`,
          `checkpoint_summary`,
          `manual_context`,
        ]),
        summary: g(y),
      }),
      S({
        ...xM,
        type: _(`fork`),
        source: T([
          S({ type: _(`run`), threadId: V, runId: G }),
          S({ type: _(`node`), nodeId: K }),
          S({ type: _(`provider_thread`), providerThreadId: Q_, providerTurnId: g($_) }),
        ]),
        targetThreadId: V,
        providerThreadId: g(Q_),
      }),
      S({
        ...xM,
        type: _(`thread_created`),
        targetThreadId: V,
        targetRunId: D(G),
        targetProviderInstanceId: q,
        targetModel: L,
      }),
      S({ ...xM, ...Uj }),
      S({
        ...xM,
        type: _(`subagent`),
        subagentId: K,
        origin: E([`provider_native`, `app_owned`]),
        driver: ny,
        providerInstanceId: q,
        childThreadId: D(V),
        prompt: y,
        progress: g(y),
        result: D(y),
      }),
      S({
        ...xM,
        type: _(`dynamic_tool`),
        toolName: D(L),
        viewedImagePath: g(L),
        input: v,
        output: g(v),
        outputOmitted: g(x),
      }),
    ]),
    CM = Jj.mapFields((e) => ({ ...e, item: SM }));
  Qj.mapFields((e) => ({
    ...e,
    thread: aM,
    runs: w(oM),
    attempts: w(sM),
    nodes: w(cM),
    subagents: w(lM),
    providerSessions: w(dM),
    providerThreads: w(pM),
    providerTurns: w(gM),
    runtimeRequests: w(_M),
    messages: w(vM),
    plans: w(kj),
    turnItems: Kj(SM),
    checkpointScopes: w(uM),
    checkpoints: w(yM),
    contextHandoffs: w(mM),
    contextTransfers: w(hM),
    visibleTurnItems: qj(
      CM,
      CM.mapFields((e) => ({ ...e, item: I_(SM.members, `type`) })),
    ),
    updatedAt: P,
  }));
  var wM = eM.mapFields((e) => ({ ...e, createdAt: P })),
    TM = tM.mapFields((e) => ({ ...e, updatedAt: P })),
    EM = nM.mapFields((e) => ({
      ...e,
      latestRunRequestedAt: g(D(P)),
      latestRunStartedAt: g(D(P)),
      latestRunCompletedAt: g(D(P)),
      activityRunStartedAt: g(D(P)),
      pendingRuntimeRequest: D(wM),
      latestVisibleMessage: D(TM),
      latestUserMessageAt: D(P),
      latestUserAuthoredMessageAt: g(D(P)),
      createdAt: P,
      updatedAt: P,
      archivedAt: D(P),
      settledAt: D(P),
      unsettledAt: g(D(P)),
      snoozedUntil: g(D(P)),
      snoozedAt: g(D(P)),
      pinnedAt: g(D(P)),
      autoSettleDisabledAt: g(D(P)),
      lastVisitedAt: g(D(P)),
      titleRegeneration: g(D(S({ requestId: W, startedAt: P }))),
      deletedAt: D(P),
    }));
  rM.mapFields((e) => ({ ...e, threads: w(EM), archivedThreads: w(EM) }));
  var DM = { ...Xj.fields, occurredAt: P };
  Yj.mapFields((e) => ({ ...e, observedAt: P }));
  var OM = T([
    S({ ...DM, type: _(`thread.created`), payload: aM }),
    S({
      ...DM,
      type: E([
        `thread.archived`,
        `thread.unarchived`,
        `thread.deleted`,
        `thread.settled`,
        `thread.unsettled`,
        `thread.snoozed`,
        `thread.unsnoozed`,
        `thread.pinned`,
        `thread.auto-settle-set`,
        `thread.unpinned`,
        `thread.pin-reordered`,
        `thread.active-reordered`,
        `thread.visited`,
        `thread.marked-unread`,
        `thread.pull-request-synced`,
        `thread.metadata-updated`,
        `thread.runtime-mode-updated`,
        `thread.interaction-mode-updated`,
        `thread.model-selection-updated`,
        `thread.provider-switched`,
      ]),
      payload: aM,
    }),
    S({ ...DM, type: _(`run.created`), payload: oM }),
    S({ ...DM, type: _(`run.updated`), payload: oM }),
    S({ ...DM, type: _(`run.background-work-cancelled`), payload: ZA }),
    S({ ...DM, type: _(`run-attempt.created`), payload: sM }),
    S({ ...DM, type: _(`run-attempt.updated`), payload: sM }),
    S({ ...DM, type: _(`node.updated`), payload: cM }),
    S({ ...DM, type: _(`subagent.updated`), payload: lM }),
    S({ ...DM, type: E([`provider-session.attached`, `provider-session.updated`]), payload: dM }),
    S({ ...DM, type: _(`provider-session.detached`), payload: fM }),
    S({ ...DM, type: _(`provider-thread.updated`), payload: pM }),
    S({ ...DM, type: _(`provider-turn.updated`), payload: gM }),
    S({ ...DM, type: _(`runtime-request.updated`), payload: _M }),
    S({ ...DM, type: _(`message.updated`), payload: vM }),
    S({ ...DM, type: _(`turn-item.updated`), payload: SM }),
    S({ ...DM, type: _(`plan.updated`), payload: kj }),
    S({ ...DM, type: _(`checkpoint-scope.created`), payload: uM }),
    S({ ...DM, type: _(`checkpoint.captured`), payload: yM }),
    S({ ...DM, type: _(`checkpoint.rollback-requested`), payload: bM }),
    S({ ...DM, type: _(`context-handoff.updated`), payload: mM }),
    S({ ...DM, type: _(`context-transfer.created`), payload: hM }),
    S({ ...DM, type: _(`context-transfer.updated`), payload: hM }),
  ]);
  S({ sequence: R, commandId: D(W), event: OM });
  var kM = T([
    S({
      type: _(`thread.create`),
      ...SA,
      commandId: W,
      threadId: V,
      projectId: H,
      title: L,
      modelSelection: bw,
      runtimeMode: VE,
      interactionMode: UE,
      branch: D(L),
      worktreePath: D(L),
      importedNativeThread: g(
        S({ ref: S({ driver: ny, nativeId: L, strength: _(`strong`) }), metadata: g(mj) }),
      ),
    }),
    S({ type: _(`thread.archive`), commandId: W, threadId: V }),
    S({ type: _(`thread.unarchive`), commandId: W, threadId: V }),
    S({ type: _(`thread.delete`), commandId: W, threadId: V }),
    S({ type: _(`thread.settle`), commandId: W, threadId: V, settledAt: g(N) }),
    S({ type: _(`thread.auto-settle`), commandId: W, threadId: V, snapshotAt: N, settledAt: g(N) }),
    S({ type: _(`thread.unsettle`), commandId: W, threadId: V, reason: _(`user`) }),
    S({ type: _(`thread.snooze`), commandId: W, threadId: V, snoozedUntil: B }),
    S({ type: _(`thread.unsnooze`), commandId: W, threadId: V, reason: _(`user`) }),
    S({ type: _(`thread.auto-settle.set`), commandId: W, threadId: V, enabled: x }),
    S({ type: _(`thread.pin`), commandId: W, threadId: V, orderKey: g(L) }),
    S({ type: _(`thread.unpin`), commandId: W, threadId: V }),
    S({ type: _(`thread.pin.reorder`), commandId: W, threadId: V, orderKey: L }),
    S({ type: _(`thread.active.reorder`), commandId: W, threadId: V, orderKey: L }),
    S({ type: _(`thread.visit`), commandId: W, threadId: V, visitedAt: B }),
    S({ type: _(`thread.mark-unread`), commandId: W, threadId: V }),
    S({
      type: _(`thread.metadata.update`),
      commandId: W,
      threadId: V,
      title: g(L),
      regenerateTitle: g(x),
      branch: g(D(L)),
      worktreePath: g(D(L)),
      expectedWorktreePath: g(D(L)),
      expectedEmpty: g(x),
      limitRecovery: g(D(WA)),
      linkedPullRequest: g(D(jE)),
    }),
    S({
      type: _(`thread.pull-request.link`),
      commandId: W,
      threadId: V,
      ...IE.fields,
      url: L,
      source: ME,
    }),
    S({ type: _(`thread.pull-request.unlink`), commandId: W, threadId: V, ...IE.fields }),
    S({
      type: _(`thread.pull-request-link.sync`),
      commandId: W,
      threadId: V,
      ...IE.fields,
      snapshot: NE,
      stack: D(FE),
    }),
    S({
      type: _(`thread.pull-request.watch`),
      commandId: W,
      threadId: V,
      ...IE.fields,
      watching: x,
      link: g(S({ url: L, source: ME })),
    }),
    S({
      type: _(`thread.pull-request.sync`),
      commandId: W,
      threadId: V,
      projectId: H,
      snapshotSequence: R,
      expected: S({
        workspaceRoot: L,
        branch: D(L),
        worktreePath: D(L),
        linkedPullRequest: D(jE),
        branchPullRequest: D(jE),
      }),
      branchPullRequest: D(jE),
      linkedPullRequest: g(jE),
    }),
    S({
      type: _(`thread.title.regeneration.complete`),
      commandId: W,
      threadId: V,
      requestId: W,
      title: g(L),
    }),
    S({ type: _(`thread.runtime-mode.set`), commandId: W, threadId: V, runtimeMode: VE }),
    S({ type: _(`thread.interaction-mode.set`), commandId: W, threadId: V, interactionMode: UE }),
    S({ type: _(`thread.model-selection.set`), commandId: W, threadId: V, modelSelection: bw }),
    S({
      type: _(`provider-session.detach`),
      commandId: W,
      threadId: V,
      providerSessionId: Z_,
      reason: g(y),
    }),
    S({
      type: _(`message.dispatch`),
      notification: g(wj),
      ...SA,
      scheduledTaskId: g(iv),
      senderThreadId: g(V),
      commandId: W,
      threadId: V,
      messageId: U_,
      text: y,
      context: g(Xv),
      attachments: w(ow),
      titleSeed: g(L),
      modelSelection: g(bw),
      sourcePlanRef: g(S({ threadId: V, planId: pv })),
      restartContinuationOfRunId: g(G),
      usageLimitContinuationOfRunId: g(G),
      manualContinuationOfRunId: g(G),
      usageLimitRecoveryRequestId: g(W),
      deliveryIntent: g(E([`auto`, `steer`, `restart`])),
      delegatedCompletion: g(S({ parentRunId: G, generation: z, taskIds: w(K) })),
      dispatchMode: T([
        S({ type: _(`defer_start`), workspaceStrategy: g(QA) }),
        S({ type: _(`steer_active`), targetRunId: G }),
        S({ type: _(`restart_active`), targetRunId: G }),
        S({ type: _(`queue_after_active`) }),
        S({ type: _(`start_immediately`) }),
      ]),
    }),
    S({ type: _(`prepared-run.release`), commandId: W, threadId: V, runId: G }),
    S({ type: _(`notification.delivery.accept`), commandId: W, threadId: V, messageId: U_ }),
    S({
      type: _(`prepared-run.progress`),
      commandId: W,
      threadId: V,
      runId: G,
      phase: E([`worktree`, `setup`]),
    }),
    S({ type: _(`prepared-run.fail`), commandId: W, threadId: V, runId: G, failure: Ij }),
    S({ type: _(`prepared-run.retry`), commandId: W, threadId: V, runId: G }),
    S({ type: _(`subagent.stop`), commandId: W, threadId: V, runId: G, subagentId: K }),
    S({
      type: _(`run.interrupt`),
      commandId: W,
      threadId: V,
      runId: G,
      reason: g(y),
      holdQueue: g(x),
    }),
    S({
      type: _(`queued-message.promote-to-steer`),
      commandId: W,
      threadId: V,
      queuedRunId: G,
      targetRunId: G,
    }),
    S({ type: _(`queue.resume`), commandId: W, threadId: V }),
    S({ type: _(`queued-run.reorder`), commandId: W, threadId: V, runId: G, beforeRunId: D(G) }),
    S({ type: _(`queued-run.cancel`), commandId: W, threadId: V, runId: G }),
    S({
      type: _(`queued-run.edit`),
      context: g(Xv),
      commandId: W,
      threadId: V,
      runId: G,
      text: y,
      attachments: g(w(ow)),
    }),
    S({
      type: _(`runtime-request.respond`),
      commandId: W,
      threadId: V,
      requestId: nv,
      decision: g(GE),
      answers: g(qE),
      attachmentsByQuestionId: g(JE),
    }),
    S({ type: _(`thread.user-input.dismiss`), commandId: W, threadId: V, requestId: nv }),
    S({
      type: _(`checkpoint.rollback`),
      restoreFiles: g(x),
      commandId: W,
      threadId: V,
      scopeId: lv,
      checkpointId: cv,
    }),
    S({
      type: _(`thread.fork`),
      ...SA,
      commandId: W,
      sourceThreadId: V,
      targetThreadId: V,
      sourcePoint: OA,
      title: g(L),
      createdAt: g(N),
    }),
    S({
      type: _(`thread.merge_back`),
      ...SA,
      commandId: W,
      sourceThreadId: V,
      targetThreadId: V,
      sourcePoint: OA,
      createdAt: g(N),
    }),
    S({
      type: _(`delegated_task.request`),
      ...SA,
      commandId: W,
      parentThreadId: V,
      parentRunId: G,
      parentNodeId: K,
      task: L,
      title: g(L),
      modelSelection: bw,
      runtimeMode: VE,
      interactionMode: UE,
      completionWake: g(E([`always`, `settled_only`])),
      createdAt: g(N),
    }),
    S({
      type: _(`delegated_task.wake-policy`),
      commandId: W,
      parentThreadId: V,
      taskId: K,
      completionWake: E([`always`, `settled_only`]),
    }),
    S({
      type: _(`delegated_task.completion-delivery.acknowledge`),
      commandId: W,
      parentThreadId: V,
      taskId: K,
      observedByRunId: D(G),
    }),
    S({
      type: _(`delegated_task.completion-delivery.dispose`),
      commandId: W,
      parentThreadId: V,
      taskId: K,
    }),
    S({
      type: _(`thread.created.record`),
      commandId: W,
      parentThreadId: V,
      parentRunId: G,
      parentNodeId: K,
      targetThreadId: V,
      targetRunId: D(G),
    }),
    S({ type: _(`provider.switch`), commandId: W, threadId: V, modelSelection: bw }),
  ]);
  T([
    S({
      type: _(`thread.pull-request-watch.sync`),
      commandId: W,
      threadId: V,
      ...IE.fields,
      startedAt: B,
      watch: D(LE),
      wake: g(S({ messageId: U_, text: y, notification: wj })),
    }),
    S({ type: _(`checkpoint.rollback.fail`), commandId: W, threadId: V, requestId: W, message: L }),
    S({
      type: _(`thread.background-work.settle`),
      commandId: W,
      threadId: V,
      providerThreadId: Q_,
      providerTurnId: $_,
    }),
    S({ type: _(`thread.stop`), commandId: W, threadId: V, reason: g(y) }),
    S({
      type: _(`secret_request.record`),
      commandId: W,
      threadId: V,
      runId: G,
      nodeId: K,
      turnItemId: tv,
      label: L,
      reason: y,
      placeholder: g(y),
      secretStatus: Hj,
    }),
  ]);
  var AM = {
      dispatchCommand: `orchestration.dispatchCommand`,
      getTurnDiff: `orchestration.getTurnDiff`,
      getFullThreadDiff: `orchestration.getFullThreadDiff`,
      searchThreads: `orchestration.searchThreads`,
      searchThread: `orchestration.searchThread`,
      searchThreadStream: `orchestration.searchThreadStream`,
      getArchivedShellSnapshot: `orchestration.getArchivedShellSnapshot`,
      getThreadProjection: `orchestration.getThreadProjection`,
      getWorkflowScript: `orchestration.getWorkflowScript`,
      stopWorkflow: `orchestration.stopWorkflow`,
      getTurnItem: `orchestration.getTurnItem`,
      launchThread: `orchestration.launchThread`,
      subscribeArchivedShell: `orchestration.subscribeArchivedShell`,
      subscribeShell: `orchestration.subscribeShell`,
      subscribeThread: `orchestration.subscribeThread`,
    },
    jM = S({ schemaVersion: z, snapshotSequence: R, projects: w(GD), threads: w(nM) }),
    MM = T([
      S({ kind: _(`snapshot`), snapshot: jM }),
      S({ kind: _(`thread.updated`), sequence: R, thread: nM }),
      S({ kind: _(`thread.removed`), sequence: R, threadId: V }),
    ]),
    NM = S({
      commandId: W,
      creationSource: g(bA),
      threadId: g(V),
      reuseExistingThread: g(x),
      projectId: H,
      title: L,
      generateTitle: g(x),
      modelSelection: bw,
      runtimeMode: VE,
      interactionMode: UE,
      workspaceStrategy: QA,
      initialMessage: g(S({ messageId: g(U_), text: y, context: g(Xv), attachments: w(ow) })),
    }),
    PM = S({ threadId: V, projection: Qj, resumed: x }),
    FM = S({ sequence: R }),
    IM = S({ threadId: V }),
    LM = S({ afterSequence: h(R), requestCompletionMarker: h(x) }),
    RM = S({
      threadId: V,
      afterSequence: h(R),
      requestCompletionMarker: h(x),
      acceptBoundedSnapshot: h(x),
      acceptCompactTurnItems: h(x),
    }),
    zM = S({
      snapshotSequence: R,
      projection: Qj,
      historyCursor: h(D(L)),
      hasMoreHistory: h(x),
      latestLocalTurnOrdinal: h(D(R)),
    }),
    BM = S({
      snapshotSequence: R,
      projection: Qj,
      historyCursor: D(L),
      hasMoreHistory: x,
      latestLocalTurnOrdinal: D(R),
      payloadBudgetExceeded: g(x),
      turnItemsOmitLocalVisible: h(_(!0)),
    }),
    VM = S({
      snapshotSequence: R,
      items: qj(
        Jj,
        Jj.mapFields((e) => ({ ...e, item: I_(Wj.members, `type`) })),
      ),
      nextCursor: D(L),
      hasMoreHistory: x,
    }),
    HM = new Set(
      Zj.members.flatMap((e) => {
        let t = e.fields.type;
        return `literals` in t ? t.literals : [t.literal];
      }),
    ),
    UM = S({
      kind: _(`event`),
      sequence: R,
      event: S({ type: y, payload: g(v) }).check(
        k(
          (e) =>
            !HM.has(e.type) ||
            (e.type === `turn-item.updated` && Gj(e.payload)) ||
            `A known event type must decode in full.`,
        ),
      ),
    }).pipe(
      xg(S({ kind: _(`unknown-event`), sequence: R, eventType: y }), {
        decode: ad((e) => ({
          kind: `unknown-event`,
          sequence: e.sequence,
          eventType: e.event.type,
        })),
        encode: td(() => `Servers never send unknown thread events.`),
      }),
    ),
    WM = T([
      S({ kind: _(`synchronized`) }),
      S({
        kind: _(`snapshot`),
        snapshotSequence: R,
        projection: Qj,
        historyCursor: h(D(L)),
        hasMoreHistory: h(x),
        latestLocalTurnOrdinal: h(D(R)),
        payloadBudgetExceeded: h(x),
        turnItemsOmitLocalVisible: h(_(!0)),
      }),
      S({ kind: _(`event`), sequence: R, event: Zj }),
      UM,
    ]),
    GM = class extends F()(`OrchestrationV2DispatchCommandError`, {
      commandId: W,
      commandType: y,
      message: y,
      detail: g(y),
      cause: g(M()),
    }) {},
    KM = class extends F()(`OrchestrationV2GetThreadProjectionError`, {
      threadId: V,
      message: y,
      cause: g(M()),
    }) {},
    qM = class extends F()(`OrchestrationV2GetShellSnapshotError`, {
      message: y,
      cause: g(M()),
    }) {},
    JM = class extends F()(`OrchestrationV2ThreadLaunchError`, {
      commandId: W,
      projectId: H,
      message: y,
      cause: g(M()),
    }) {},
    YM = S({ threadId: V, subagentId: K }),
    XM = class extends F()(`OrchestrationV2StopWorkflowError`, {
      threadId: V,
      subagentId: K,
      reason: E([`not-running`, `unsupported`, `unavailable`, `stop-failed`]),
      cause: g(M()),
    }) {
      get message() {
        switch (this.reason) {
          case `not-running`:
            return `This workflow is no longer running.`;
          case `unsupported`:
            return `This provider cannot stop a workflow.`;
          case `unavailable`:
            return `This workflow has no live provider task.`;
          case `stop-failed`:
            return `Failed to stop the workflow.`;
        }
      }
    };
  T([GM, KM, qM, JM, XM]);
  var ZM = S({ threadId: V, scriptPath: L }),
    QM = S({ scriptPath: L, contents: y, truncated: x });
  F()(`OrchestrationWorkflowFileError`, {
    reason: E([
      `invalid-path`,
      `root-unavailable`,
      `not-found`,
      `outside-root`,
      `wrong-extension`,
      `not-regular-file`,
      `changed-during-read`,
      `read-failed`,
    ]),
    path: y,
    cause: g(M()),
  });
  var $M = S({ threadId: V, itemId: tv, revision: g(y) }),
    eN = S({ item: D(Wj) }),
    tN = {
      "invalid-path": `Workflow scripts must be absolute .js paths.`,
      "root-unavailable": `Script root unavailable.`,
      "not-found": `Script not found.`,
      "outside-root": `Script path is outside the workflow scripts root.`,
      "not-js": `Resolved script is not a .js file.`,
      "not-regular-file": `Script is not a regular file.`,
      "changed-during-read": `Script changed between resolution and open.`,
      "read-failed": `Script read failed.`,
    },
    nN = class extends F()(`OrchestrationGetWorkflowScriptError`, {
      reason: E([
        `invalid-path`,
        `root-unavailable`,
        `not-found`,
        `outside-root`,
        `not-js`,
        `not-regular-file`,
        `changed-during-read`,
        `read-failed`,
      ]),
      scriptPath: y,
      cause: g(M()),
    }) {
      get message() {
        return tN[this.reason];
      }
    },
    rN = S({
      threadId: V,
      skills: h(w(S({ name: y.check(j(200)), displayName: g(y.check(j(200))) })).check(j(1e3))),
      query: L.check(j(200)),
      index: h(R),
      offset: h(A),
      start: h(S({ entryId: L, occurrence: R })),
    }),
    iN = S({ entryId: L, runId: D(G), occurrence: R }),
    aN = S({
      complete: h(x),
      snapshotSequence: R,
      totalMatches: R,
      activeIndex: R,
      match: D(iN),
      navigation: h(w(S({ entryId: L, runId: D(G), startIndex: R, count: R })).check(j(17))),
    }),
    oN = class extends F()(`OrchestrationV2SearchThreadError`, { cause: M() }) {
      get message() {
        return `Could not search this thread. Please retry.`;
      }
    },
    sN = {
      searchThread: { input: rN, output: aN },
      dispatchCommand: { input: kM, output: FM },
      getTurnDiff: { input: pw, output: mw },
      getFullThreadDiff: { input: hw, output: gw },
      getArchivedShellSnapshot: { input: S({}), output: jM },
      getThreadProjection: { input: IM, output: Qj },
      stopWorkflow: { input: YM, output: sg },
      getWorkflowScript: { input: ZM, output: QM },
      getTurnItem: { input: $M, output: eN },
      launchThread: { input: NM, output: PM },
      subscribeArchivedShell: { input: S({}), output: MM },
      subscribeShell: { input: LM, output: iM },
      subscribeThread: { input: RM, output: WM },
    },
    cN = T([
      S({ type: _(`expect_outbound`), label: g(L), frame: v }),
      S({ type: _(`emit_inbound`), label: g(L), frame: v, afterMs: g(R) }),
      S({ type: _(`runtime_exit`), status: E([`success`, `error`, `cancelled`]), error: g(v) }),
    ]);
  (S({ provider: L, protocol: L, version: L, scenario: L, metadata: g(C(y, v)), entries: w(cN) }),
    T([
      S({
        type: _(`transcript_start`),
        provider: L,
        protocol: L,
        version: L,
        scenario: L,
        metadata: g(C(y, v)),
      }),
      cN,
    ]));
  var lN = `~effect/http-api/HttpApiSecurity`,
    uN = {
      [lN]: lN,
      pipe() {
        return n(this, arguments);
      },
    },
    dN = (e) =>
      Object.assign(Object.create(uN), {
        _tag: `Http`,
        scheme: e.scheme,
        schemeLength: e.scheme.length,
        annotations: Ar(),
      }),
    fN = a(3, (e, t, n) =>
      Object.assign(Object.create(uN), { ...e, annotations: Nr(e.annotations, t, n) }),
    ),
    pN = class extends fr()(`effect/http-api/OpenApi/Title`) {},
    mN = class extends fr()(`effect/http-api/OpenApi/Version`) {},
    hN = class extends fr()(`effect/http-api/OpenApi/Description`) {},
    gN = class extends fr()(`effect/http-api/OpenApi/Summary`) {},
    _N = E([`ios`, `android`]),
    vN = E([
      `starting`,
      `running`,
      `waiting_for_approval`,
      `waiting_for_input`,
      `completed`,
      `failed`,
      `stale`,
    ]),
    yN = S({
      liveActivitiesEnabled: x,
      notificationsEnabled: x,
      notifyOnApproval: x,
      notifyOnInput: x,
      notifyOnCompletion: x,
      notifyOnFailure: x,
    }),
    bN = E([`sandbox`, `production`]),
    xN = S({
      deviceId: L,
      label: L,
      platform: _N,
      iosMajorVersion: g(A.check(Hg(18))),
      androidApiLevel: g(A.check(Hg(24))),
      appVersion: g(L),
      bundleId: g(L),
      apsEnvironment: g(bN),
      pushToken: g(L),
      pushToStartToken: g(L),
      preferences: yN,
    }).check(
      k((e) =>
        e.platform === `ios`
          ? e.iosMajorVersion !== void 0
          : e.androidApiLevel !== void 0 &&
            e.iosMajorVersion === void 0 &&
            e.apsEnvironment === void 0 &&
            e.pushToStartToken === void 0,
      ),
    ),
    SN = S({
      deviceId: L,
      label: L,
      platform: _N,
      iosMajorVersion: D(A.check(Hg(18))),
      androidApiLevel: g(D(A)),
      appVersion: D(L),
      notifications: S({
        enabled: x,
        notifyOnApproval: x,
        notifyOnInput: x,
        notifyOnCompletion: x,
        notifyOnFailure: x,
      }),
      liveActivities: S({ enabled: x }),
      updatedAt: L,
    }),
    CN = S({ devices: w(SN) }),
    wN = S({
      devices: w(S({ ...SN.fields, platform: _(`ios`), iosMajorVersion: A.check(Hg(18)) })),
    }),
    TN = S({ deviceId: L, activityPushToken: L }),
    EN = S({ deviceId: L }),
    DN = S({
      environmentId: U,
      threadId: V,
      projectTitle: L,
      threadTitle: L,
      phase: vN,
      headline: L,
      detail: g(L),
      modelTitle: L,
      updatedAt: L,
      deepLink: L,
    }),
    ON = S({
      environmentId: U,
      threadId: V,
      projectTitle: L,
      threadTitle: L,
      modelTitle: L,
      phase: vN,
      status: L,
      updatedAt: L,
      deepLink: L,
    }),
    kN = S({ title: L, subtitle: L, activeCount: A.check(Hg(0)), updatedAt: L, activities: w(ON) }),
    AN = E([`manual`, `cloudflare_tunnel`, `t3_relay`]),
    jN = S({ httpBaseUrl: L, wsBaseUrl: L, providerKind: AN }),
    MN = S({ localHttpHost: L, localHttpPort: A.check(Hg(1), Wg(65535)) }),
    NN = S({ providerKind: AN, connectorToken: L, tunnelId: g(L), tunnelName: g(L) }),
    PN = S({ cloudUserId: L, origin: MN, proof: L }),
    FN = S({ cloudUserId: L, tunnelId: L, origin: MN, proof: L }),
    IN = S({ status: E([`ready`, `recovery_required`]) }),
    LN = S({ endpoint: jN, endpointRuntime: NN }),
    RN = S({ challenge: y, relayIssuer: y, endpoint: jN, origin: MN }),
    zN = S({
      relayUrl: y,
      relayIssuer: g(y),
      cloudUserId: y,
      environmentCredential: y,
      cloudMintPublicKey: y,
      endpointRuntime: D(NN),
    }),
    BN = { iss: L, aud: L, sub: L, jti: L, iat: A, exp: A };
  (T([
    S({ ...BN, action: _(`register`), environmentId: U, cloudUserId: L, tunnelId: L, origin: MN }),
    S({ ...BN, action: _(`recover`), environmentId: U, cloudUserId: L, origin: MN }),
  ]),
    S({ ...BN, environmentId: U, threadId: V, state: D(DN) }));
  var VN = S({
      state: D(DN).annotate({
        description: `Current agent-awareness state, or null to remove the published state.`,
      }),
      proof: L.annotate({
        description: `Environment-signed JWT covering this published activity state.`,
      }),
    }).annotate({ description: `Publishes a signed agent-awareness update from an environment.` }),
    HN = E([`agent_activity_notifications`, `managed_tunnels`]);
  S({
    ...BN,
    challenge: L,
    descriptor: dS,
    environmentId: U,
    environmentPublicKey: L,
    endpoint: jN,
    origin: MN,
    scopes: w(HN),
  });
  var UN = L,
    WN = S({
      notificationsEnabled: x.annotate({
        description: `Whether this link may deliver push notifications.`,
      }),
      liveActivitiesEnabled: x.annotate({
        description: `Whether this link may update Live Activities.`,
      }),
      managedTunnelsEnabled: x.annotate({
        description: `Whether the relay should provision a managed tunnel for this environment.`,
      }),
    }).annotate({ description: `Requested capabilities for a new environment-link challenge.` }),
    GN = S({ challenge: L, expiresAt: L }),
    KN = S({
      deviceId: g(
        L.annotate({ description: `Optional client device identifier associated with this link.` }),
      ),
      proof: UN.annotate({
        description: `Environment-signed proof bound to a previously issued link challenge.`,
      }),
      notificationsEnabled: x,
      liveActivitiesEnabled: x,
      managedTunnelsEnabled: x,
    }).annotate({ description: `Links an authenticated cloud user to a T3 environment.` }),
    qN = S({
      ok: x,
      cloudUserId: L,
      environmentId: U,
      endpoint: jN,
      endpointRuntime: D(NN),
      relayIssuer: L,
      environmentCredential: L,
      cloudMintPublicKey: L,
    }),
    JN = E([
      `invalid_signature_or_scope`,
      `descriptor_mismatch`,
      `replayed_nonce`,
      `challenge_invalid`,
      `origin_not_allowed`,
      `endpoint_not_secure`,
    ]),
    YN = E([
      `link_persistence_failed`,
      `credential_persistence_failed`,
      `replay_persistence_failed`,
      `internal_error`,
    ]),
    XN = E([`managed_endpoint_not_configured`, `managed_endpoint_provisioning_failed`]),
    ZN = E([`endpoint_request_failed`, `endpoint_response_invalid`]),
    QN = E([`invalid_signature_or_payload`, `replayed_nonce`]),
    $N = E([`missing_bearer`, `invalid_bearer`, `invalid_dpop`, `not_authorized`]),
    eP = A_,
    tP = E([
      `database_unavailable`,
      `persistence_failed`,
      `upstream_unavailable`,
      `internal_error`,
    ]),
    nP = class extends F()(
      `RelayAuthInvalidError`,
      { code: _(`auth_invalid`), reason: $N, dpopFailureReason: h(eP), traceId: L },
      { httpApiStatus: 401 },
    ) {
      get message() {
        return `Relay authentication failed: ${this.reason}`;
      }
    },
    rP = class extends F()(
      `RelayEnvironmentLinkProofExpiredError`,
      { code: _(`environment_link_proof_expired`), traceId: L },
      { httpApiStatus: 401 },
    ) {
      get message() {
        return `Relay environment link proof expired`;
      }
    },
    iP = class extends F()(
      `RelayEnvironmentLinkProofInvalidError`,
      { code: _(`environment_link_proof_invalid`), reason: JN, traceId: L },
      { httpApiStatus: 400 },
    ) {
      get message() {
        return `Relay environment link proof is invalid: ${this.reason}`;
      }
    },
    aP = E([
      `client_proof_key_thumbprint_missing`,
      `environment_link_not_found`,
      `endpoint_provider_not_managed`,
      `managed_endpoint_allocation_not_found`,
      `managed_endpoint_base_domain_not_configured`,
      `managed_endpoint_allocation_not_ready`,
      `managed_endpoint_hostname_invalid`,
      `managed_endpoint_mismatch`,
    ]),
    oP = class extends F()(
      `RelayEnvironmentConnectNotAuthorizedError`,
      { code: _(`environment_connect_not_authorized`), reason: g(aP), traceId: L },
      { httpApiStatus: 403 },
    ) {
      get message() {
        return this.reason
          ? `Relay environment connection is not authorized: ${this.reason}`
          : `Relay environment connection is not authorized`;
      }
    },
    sP = class extends F()(
      `RelayEnvironmentEndpointUnavailableError`,
      { code: _(`environment_endpoint_unavailable`), reason: ZN, traceId: L },
      { httpApiStatus: 502 },
    ) {
      get message() {
        return `Relay environment endpoint is unavailable: ${this.reason}`;
      }
    },
    cP = class extends F()(
      `RelayEnvironmentEndpointTimedOutError`,
      { code: _(`environment_endpoint_timed_out`), traceId: L },
      { httpApiStatus: 504 },
    ) {
      get message() {
        return `Relay environment endpoint request timed out`;
      }
    },
    lP = class extends F()(
      `RelayEnvironmentLinkFailedError`,
      { code: _(`environment_link_failed`), reason: YN, traceId: L },
      { httpApiStatus: 500 },
    ) {
      get message() {
        return `Relay environment link failed: ${this.reason}`;
      }
    },
    uP = class extends F()(
      `RelayEnvironmentLinkUnavailableError`,
      { code: _(`environment_link_unavailable`), reason: XN, traceId: L },
      { httpApiStatus: 503 },
    ) {
      get message() {
        return `Relay environment link is unavailable: ${this.reason}`;
      }
    },
    dP = class extends F()(
      `RelayEnvironmentLinkLimitExceededError`,
      { code: _(`environment_link_limit_exceeded`), maxTunnels: b, traceId: L },
      { httpApiStatus: 403 },
    ) {
      get message() {
        return `Relay managed tunnel limit reached: this account allows at most ${this.maxTunnels} tunnels`;
      }
    },
    fP = class extends F()(
      `RelayAgentActivityPublishProofExpiredError`,
      { code: _(`agent_activity_publish_proof_expired`), traceId: L },
      { httpApiStatus: 401 },
    ) {
      get message() {
        return `Relay agent activity publish proof expired`;
      }
    },
    pP = class extends F()(
      `RelayAgentActivityPublishProofInvalidError`,
      { code: _(`agent_activity_publish_proof_invalid`), reason: QN, traceId: L },
      { httpApiStatus: 401 },
    ) {
      get message() {
        return `Relay agent activity publish proof is invalid: ${this.reason}`;
      }
    },
    mP = class extends F()(
      `RelayInternalError`,
      { code: _(`internal_error`), reason: tP, traceId: L },
      { httpApiStatus: 500 },
    ) {
      get message() {
        return `Relay internal error: ${this.reason}`;
      }
    };
  T([nP, rP, iP, oP, sP, cP, lP, uP, dP, fP, pP, mP]);
  var hP = [nP, mP],
    gP = [nP, rP, iP, uP, dP, lP, mP],
    _P = [nP, oP, sP, cP, mP],
    vP = [nP, fP, pP, mP];
  (fr()(`@t3tools/contracts/relay/RelayClientPrincipal`),
    fr()(`@t3tools/contracts/relay/RelayEnvironmentPrincipal`));
  var yP = dN({ scheme: `bearer` }).pipe(
      fN(hN, `Clerk session or OAuth bearer token for the signed-in T3 Connect user.`),
    ),
    bP = class extends LC()(`RelayClientAuth`, { error: nP, security: { clientBearer: yP } }) {},
    xP = dN({ scheme: `bearer` }).pipe(
      fN(hN, `Relay-issued environment credential installed when the environment is linked.`),
    ),
    SP = class extends LC()(`RelayEnvironmentAuth`, {
      error: [nP, mP],
      security: { environmentBearer: xP },
    }) {},
    CP = dN({ scheme: `DPoP` }).pipe(
      fN(hN, `DPoP-bound access token. Requests must also include the DPoP proof JWT header.`),
    ),
    wP = class extends LC()(`RelayDpopClientAuth`, { error: nP, security: { relayDpop: CP } }) {},
    TP = S({ environments: w(S({ environmentId: U, label: L, endpoint: jN, linkedAt: L })) }),
    EP = S({
      deviceId: g(
        L.annotate({ description: `Optional client device identifier requesting the connection.` }),
      ),
      clientKeyThumbprint: g(
        L.annotate({ description: `Deprecated alias for clientProofKeyThumbprint.` }),
      ),
      clientProofKeyThumbprint: g(
        L.annotate({
          description: `JWK thumbprint that the minted environment credential must be bound to.`,
        }),
      ),
    }).annotate({
      description: `Requests a short-lived credential for connecting to an environment.`,
    }),
    DP = E([`environment:connect`, `environment:status`, `mobile:registration`]),
    OP = `urn:ietf:params:oauth:grant-type:token-exchange`,
    kP = `urn:ietf:params:oauth:token-type:jwt`,
    AP = `urn:ietf:params:oauth:token-type:access_token`,
    jP = E([`t3-mobile`, `t3-web`]),
    MP = S({
      grant_type: _(OP),
      subject_token: L.annotate({
        description: `Clerk bearer token for the signed-in cloud user.`,
      }),
      subject_token_type: _(kP),
      requested_token_type: _(AP),
      resource: L.annotate({
        description: `Relay issuer URL that will receive the DPoP-bound access token.`,
      }),
      scope: L.annotate({ description: `Space-separated relay scopes requested by the client.` }),
      client_id: jP,
    })
      .annotate({
        description: `OAuth token exchange request for a DPoP-bound relay access token.`,
      })
      .pipe(Fb()),
    NP = S({
      access_token: L,
      issued_token_type: _(AP),
      token_type: _(`DPoP`),
      expires_in: A.check(Vg(0)),
      scope: L,
    }),
    PP = S({ authorization: L }),
    FP = S({ dpop: L }),
    IP = S({ authorization: L, dpop: L }),
    LP = S({
      issuer: L,
      token_endpoint: L,
      grant_types_supported: w(_(OP)),
      token_endpoint_auth_methods_supported: w(_(`none`)),
      dpop_signing_alg_values_supported: w(_(`ES256`)),
      scopes_supported: w(DP),
    }),
    RP = S({
      resource: L,
      authorization_servers: w(L),
      scopes_supported: w(DP),
      dpop_bound_access_tokens_required: x,
      dpop_signing_alg_values_supported: w(_(`ES256`)),
    }),
    zP = S({ environmentId: U }),
    BP = S({ environmentId: U, endpoint: jN, credential: L, expiresAt: L }),
    VP = E([`online`, `offline`]),
    HP = E([`tunnel_released`]),
    UP = S({
      environmentId: U,
      endpoint: jN,
      status: VP,
      checkedAt: L,
      descriptor: g(dS),
      error: g(L),
      offlineReason: g(HP),
      traceId: g(L),
    });
  S({
    ...BN,
    environmentId: U,
    clientProofKeyThumbprint: L,
    cnf: S({ jkt: L }),
    deviceId: g(L),
    nonce: L,
    scope: w(_(`environment:connect`)),
  });
  var WP = S({ proof: L });
  (S({ ...BN, environmentId: U, nonce: L, scope: w(_(`environment:status`)) }),
    S({ ...BN, environmentId: U, deliveryId: L, receivedAt: L, hookId: L }));
  var GP = S({ proof: L });
  S({
    ...BN,
    environmentId: U,
    requestNonce: L,
    status: _(`online`),
    descriptor: dS,
    checkedAt: L,
  });
  var KP = S({ environmentId: U, status: _(`online`), descriptor: dS, checkedAt: L, proof: L });
  S({ ...BN, environmentId: U, clientProofKeyThumbprint: L, requestNonce: L, credential: L });
  var qP = S({ credential: L, expiresAt: L, proof: L }),
    JP = S({
      deviceId: L,
      kind: E([
        `live_activity_start`,
        `live_activity_update`,
        `live_activity_end`,
        `push_notification`,
      ]),
      ok: x,
      queued: g(x),
      apnsStatus: D(b),
      apnsReason: D(y),
      apnsId: D(y),
    }),
    YP = S({ ok: x }),
    XP = S({ ok: x, deliveries: w(JP) }),
    ZP = S({ ok: x, service: _(`relay`) }),
    QP = PC(`health`)
      .add(bC(`health`, `/health`, { success: ZP, error: mP }).annotate(gN, `Check relay health`))
      .annotate(hN, `Service health and readiness.`),
    $P = PC(`metadata`)
      .add(
        bC(`authorizationServer`, `/.well-known/oauth-authorization-server`, {
          success: LP,
        }).annotate(gN, `Read OAuth authorization-server metadata`),
        bC(`protectedResource`, `/.well-known/oauth-protected-resource`, { success: RP }).annotate(
          gN,
          `Read OAuth protected-resource metadata`,
        ),
      )
      .annotate(hN, `OAuth and DPoP discovery metadata.`),
    eF = xC(`registerDevice`, `/v1/mobile/devices`, {
      headers: IP,
      payload: xN,
      success: YP,
      error: hP,
    }).annotate(gN, `Register or update a mobile device`),
    tF = xC(`registerLiveActivity`, `/v1/mobile/live-activities`, {
      headers: IP,
      payload: TN,
      success: YP,
      error: hP,
    }).annotate(gN, `Register a Live Activity push token`),
    nF = bC(`getAgentActivitySnapshot`, `/v1/mobile/agent-activity`, {
      headers: IP,
      success: S({ aggregate: D(kN) }),
      error: hP,
    }).annotate(gN, `Read the current Live Activity aggregate`),
    rF = wC(`unregisterDevice`, `/v1/mobile/devices/:deviceId`, {
      headers: IP,
      params: EN,
      success: YP,
      error: hP,
    }).annotate(gN, `Unregister a mobile device`),
    iF = PC(`mobile`)
      .add(eF, tF, nF, rF)
      .annotate(hN, `Mobile push-notification and Live Activity registration.`)
      .middleware(wP),
    aF = PC(`client`)
      .add(
        bC(`listEnvironments`, `/v1/environments`, {
          headers: PP,
          success: TP,
          error: hP,
        }).annotate(gN, `List linked environments`),
        bC(`listDevices`, `/v1/client/devices`, { headers: PP, success: wN, error: hP }).annotate(
          gN,
          `List registered iOS devices (legacy clients)`,
        ),
        bC(`listDevicesV2`, `/v2/client/devices`, { headers: PP, success: CN, error: hP }).annotate(
          gN,
          `List registered mobile devices`,
        ),
        xC(`linkEnvironment`, `/v1/client/environment-links`, {
          headers: PP,
          payload: KN,
          success: qN,
          error: gP,
        }).annotate(gN, `Link an environment`),
        xC(`createEnvironmentLinkChallenge`, `/v1/client/environment-link-challenges`, {
          headers: PP,
          payload: WN,
          success: GN,
          error: hP,
        }).annotate(gN, `Create an environment-link challenge`),
        wC(`unlinkEnvironment`, `/v1/client/environment-links/:environmentId`, {
          headers: PP,
          params: zP,
          success: YP,
          error: hP,
        }).annotate(gN, `Unlink an environment`),
        wC(`releaseEnvironmentTunnel`, `/v1/client/environment-links/:environmentId/tunnel`, {
          headers: PP,
          params: zP,
          success: YP,
          error: hP,
        })
          .annotate(gN, `Release an environment's managed tunnel`)
          .annotate(
            hN,
            `Deletes the provisioned Cloudflare tunnel while keeping the environment link and its hostname reservation, so a later link re-provisions the tunnel under the same URL. Environments call this when they shut down; Cloudflare bills per provisioned tunnel, so idle tunnels should not outlive their environment.`,
          ),
      )
      .annotate(hN, `Cloud-user environment links and registered devices.`)
      .middleware(bP),
    oF = xC(`exchangeDpopAccessToken`, `/v1/client/dpop-token`, {
      headers: FP,
      payload: MP,
      success: NP,
      error: hP,
    })
      .annotate(gN, `Exchange a Clerk token for a DPoP access token`)
      .annotate(
        hN,
        `Bootstrap endpoint. Send the DPoP proof JWT in the dpop header and the Clerk token in subject_token. The returned access token is bound to the proof key.`,
      ),
    sF = PC(`token`).add(oF).annotate(hN, `OAuth token exchange for DPoP-bound client access.`),
    cF = xC(`connectEnvironment`, `/v1/environments/:environmentId/connect`, {
      headers: IP,
      params: S({ environmentId: U }),
      payload: EP,
      success: BP,
      error: _P,
    }).annotate(gN, `Connect to an environment`),
    lF = xC(`getEnvironmentStatus`, `/v1/environments/:environmentId/status`, {
      headers: IP,
      params: S({ environmentId: U }),
      success: UP,
      error: _P,
    }).annotate(gN, `Check environment status`),
    uF = PC(`dpopClient`)
      .add(cF, lF)
      .annotate(hN, `DPoP-authenticated client access to linked environments.`)
      .middleware(wP),
    dF = S({ holdWebhooksWhileOffline: x }),
    fF = S({ pending: x }),
    pF = PC(`server`)
      .add(
        xC(`registerManagedEndpointRecovery`, `/v1/environments/:environmentId/tunnel/recovery`, {
          params: S({ environmentId: U }),
          payload: FN,
          success: IN,
          error: hP,
        }).annotate(gN, `Register managed tunnel recovery without provisioning`),
        xC(`recoverManagedEndpoint`, `/v1/environments/:environmentId/tunnel`, {
          params: S({ environmentId: U }),
          payload: PN,
          success: LN,
          error: hP,
        }).annotate(gN, `Recover an environment's managed tunnel`),
        xC(
          `publishAgentActivity`,
          `/v1/environments/:environmentId/threads/:threadId/agent-activity`,
          { params: S({ environmentId: U, threadId: V }), payload: VN, success: XP, error: vP },
        ).annotate(gN, `Publish agent activity`),
        xC(`updateLinkPreferences`, `/v1/environments/:environmentId/link-preferences`, {
          params: S({ environmentId: U }),
          payload: dF,
          success: dF,
          error: hP,
        }).annotate(gN, `Update an environment's link preferences`),
        xC(`wakeHeldHooks`, `/v1/environments/:environmentId/hooks/wake`, {
          params: S({ environmentId: U }),
          success: fF,
          error: hP,
        }).annotate(gN, `Deliver webhook requests held while the environment was offline now`),
      )
      .annotate(hN, `Environment-authenticated activity publication.`)
      .middleware(SP),
    mF = S({ endpointKey: y, hookId: y, token: y }),
    hF = `/v1/hooks/:endpointKey/:hookId/:token`,
    gF = { params: mF },
    _F = PC(`hooks`)
      .add(
        xC(`forwardPost`, hF, gF),
        SC(`forwardPut`, hF, gF),
        CC(`forwardPatch`, hF, gF),
        bC(`forwardGet`, hF, gF),
      )
      .annotate(hN, `Forward webhook requests to an environment.`);
  kC(`RelayApi`)
    .add(QP, $P, iF, aF, sF, uF, pF, _F)
    .annotate(pN, `S5 Code Relay API`)
    .annotate(mN, `1.0.0`)
    .annotate(
      hN,
      `Control-plane API for linking T3 environments, connecting authorized clients, and publishing agent activity.`,
    );
  var vF = S({ authorization: h(y), dpop: h(y) }),
    yF = S({ authorization: h(y), dpop: h(y), [eS]: _(`2`) }),
    bF = S({ dpop: h(y) }),
    xF = E([`invalid_scope`, `scope_not_granted`, `invalid_command`, `invalid_history_cursor`]),
    SF = E([`missing_credential`, `invalid_credential`]),
    CF = E([`current_session_revoke_not_allowed`]),
    wF = E([
      `bootstrap_validation_failed`,
      `browser_session_issuance_failed`,
      `browser_session_cookie_failed`,
      `access_token_issuance_failed`,
      `websocket_ticket_issuance_failed`,
      `pairing_credential_issuance_failed`,
      `pairing_links_load_failed`,
      `pairing_link_revoke_failed`,
      `client_sessions_load_failed`,
      `client_session_revoke_failed`,
      `project_snapshot_failed`,
      `project_mutation_failed`,
      `orchestration_snapshot_failed`,
      `orchestration_thread_snapshot_failed`,
      `orchestration_thread_bounded_snapshot_failed`,
      `orchestration_thread_history_failed`,
      `internal_error`,
    ]),
    TF = class e extends F()(
      `EnvironmentRequestInvalidError`,
      { code: _(`invalid_request`), reason: xF, traceId: L },
      { httpApiStatus: 400 },
    ) {
      [FS]() {
        return MS(e)(this, { status: 400 });
      }
      get message() {
        return `The environment rejected the request (${this.reason}).`;
      }
    },
    EF = class e extends F()(
      `EnvironmentAuthInvalidError`,
      { code: _(`auth_invalid`), reason: SF, dpopFailureReason: h(A_), traceId: L },
      { httpApiStatus: 401 },
    ) {
      [FS]() {
        return MS(e)(this, { status: 401 });
      }
      get message() {
        return `The environment rejected this client's credentials (${this.reason}).`;
      }
    },
    DF = class e extends F()(
      `EnvironmentScopeRequiredError`,
      { code: _(`insufficient_scope`), requiredScope: fx, requiredPermission: h(y), traceId: L },
      { httpApiStatus: 403 },
    ) {
      [FS]() {
        return MS(e)(this, { status: 403 });
      }
      get message() {
        return `This request needs the ${this.requiredPermission ?? this.requiredScope} scope, which this client does not have.`;
      }
    },
    OF = class e extends F()(
      `EnvironmentOperationForbiddenError`,
      { code: _(`operation_forbidden`), reason: CF, traceId: L },
      { httpApiStatus: 403 },
    ) {
      [FS]() {
        return MS(e)(this, { status: 403 });
      }
      get message() {
        return `The environment refused this operation (${this.reason}).`;
      }
    },
    kF = class e extends F()(
      `EnvironmentInternalError`,
      { code: _(`internal_error`), reason: wF, traceId: L },
      { httpApiStatus: 500 },
    ) {
      [FS]() {
        return MS(e)(this, { status: 500 });
      }
      get message() {
        return `The environment failed to answer this request (${this.reason}).`;
      }
    },
    AF = E([`thread_not_found`]),
    jF = class e extends F()(
      `EnvironmentResourceNotFoundError`,
      { code: _(`not_found`), reason: AF, traceId: L },
      { httpApiStatus: 404 },
    ) {
      [FS]() {
        return MS(e)(this, { status: 404 });
      }
      get message() {
        return `The environment could not find what this request named (${this.reason}).`;
      }
    };
  T([TF, EF, DF, OF, jF, kF]);
  var MF = [EF, kF],
    NF = class e extends F()(
      `EnvironmentHttpBadRequestError`,
      { message: y },
      { httpApiStatus: 400 },
    ) {
      [FS]() {
        return MS(e)(this, { status: 400 });
      }
    },
    PF = class e extends F()(
      `EnvironmentHttpUnauthorizedError`,
      { message: y },
      { httpApiStatus: 401 },
    ) {
      [FS]() {
        return MS(e)(this, { status: 401 });
      }
    },
    FF = class e extends F()(
      `EnvironmentHttpForbiddenError`,
      { message: y },
      { httpApiStatus: 403 },
    ) {
      [FS]() {
        return MS(e)(this, { status: 403 });
      }
    },
    IF = class e extends F()(
      `EnvironmentHttpInternalServerError`,
      { message: y },
      { httpApiStatus: 500 },
    ) {
      [FS]() {
        return MS(e)(this, { status: 500 });
      }
    },
    LF = class e extends F()(
      `EnvironmentHttpConflictError`,
      { message: y },
      { httpApiStatus: 409 },
    ) {
      [FS]() {
        return MS(e)(this, { status: 409 });
      }
    },
    RF = class e extends F()(
      `EnvironmentCloudEndpointUnavailableError`,
      { message: y, endpointRuntimeStatus: v },
      { httpApiStatus: 503 },
    ) {
      [FS]() {
        return MS(e)(this, { status: 503 });
      }
    },
    zF = [EF, kF],
    BF = [TF, EF, kF],
    VF = [DF, kF],
    HF = [TF, ...VF],
    UF = [DF, OF, kF],
    WF = [DF, kF],
    GF = [DF, kF],
    KF = [DF, jF, kF],
    qF = [TF, DF, kF];
  fr()(`@t3tools/contracts/environmentHttp/EnvironmentAuthenticatedPrincipal`);
  var JF = class extends LC()(`EnvironmentAuthenticatedAuth`, { error: MF }) {},
    YF = [NF, PF, FF, LF, IF, DF],
    XF = S({ ok: x, endpointRuntimeStatus: v }),
    ZF = S({
      linked: x,
      cloudUserId: D(y),
      relayUrl: D(y),
      relayIssuer: D(y),
      managedTunnelActive: g(x),
      publishAgentActivity: x,
      holdWebhooksWhileOffline: g(x),
    }),
    QF = S({ publishAgentActivity: x, holdWebhooksWhileOffline: g(x) }),
    $F = S({ revoked: x }),
    eI = S({ revoked: x }),
    tI = S({ revokedCount: b }),
    nI = class extends PC(`metadata`).add(
      bC(`descriptor`, `/.well-known/t3/environment`, { success: dS }),
    ) {},
    rI = class extends PC(`auth`)
      .add(bC(`session`, `/api/auth/session`, { headers: vF, success: zx, error: [kF] }))
      .add(
        xC(`browserSession`, `/api/auth/browser-session`, { payload: bx, success: xx, error: zF }),
      )
      .add(xC(`token`, `/oauth/token`, { headers: bF, payload: Cx, success: wx, error: BF }))
      .add(
        xC(`webSocketTicket`, `/api/auth/websocket-ticket`, {
          headers: vF,
          success: Tx,
          error: [kF],
        }).middleware(JF),
      )
      .add(
        xC(`pairingCredential`, `/api/auth/pairing-token`, {
          headers: vF,
          payload: Rx,
          success: Ex,
          error: HF,
        }).middleware(JF),
      )
      .add(
        bC(`pairingLinks`, `/api/auth/pairing-links`, {
          headers: vF,
          success: w(Dx),
          error: VF,
        }).middleware(JF),
      )
      .add(
        xC(`revokePairingLink`, `/api/auth/pairing-links/revoke`, {
          headers: vF,
          payload: Ix,
          success: $F,
          error: VF,
        }).middleware(JF),
      )
      .add(
        bC(`clients`, `/api/auth/clients`, { headers: vF, success: w(kx), error: VF }).middleware(
          JF,
        ),
      )
      .add(
        xC(`revokeClient`, `/api/auth/clients/revoke`, {
          headers: vF,
          payload: Lx,
          success: eI,
          error: UF,
        }).middleware(JF),
      )
      .add(
        xC(`revokeOtherClients`, `/api/auth/clients/revoke-others`, {
          headers: vF,
          success: tI,
          error: VF,
        }).middleware(JF),
      ) {},
    iI = class extends PC(`mcpOAuth`)
      .add(bC(`protectedResource`, `/.well-known/oauth-protected-resource`, { success: Vx }))
      .add(bC(`mcpProtectedResource`, `/.well-known/oauth-protected-resource/mcp`, { success: Vx }))
      .add(bC(`authorizationServer`, `/.well-known/oauth-authorization-server`, { success: Hx }))
      .add(xC(`register`, `/oauth/mcp/register`, { payload: Ux, success: Wx, error: Gx }))
      .add(bC(`authorize`, `/oauth/mcp/authorize`))
      .add(xC(`approval`, `/oauth/mcp/approval`, { payload: Kx, success: [qx, Jx], error: Xx }))
      .add(xC(`decision`, `/oauth/mcp/decision`, { payload: Yx, success: Jx, error: Xx }))
      .add(xC(`token`, `/oauth/mcp/token`, { payload: Zx, success: Qx, error: $x })) {},
    aI = S({ threadId: V }),
    oI = S({ compactTurnItems: h(y) }),
    sI = S({ cursor: L, throughEntryId: g(L), view: g(E([`conversation`, `activity`])) }),
    cI = [TF, DF, jF, kF],
    lI = class extends PC(`orchestration`)
      .add(
        bC(`shellSnapshot`, `/api/orchestration/shell`, {
          headers: yF,
          success: rM,
          error: GF,
        }).middleware(JF),
      )
      .add(
        bC(`threadSnapshot`, `/api/orchestration/threads/:threadId`, {
          headers: yF,
          params: aI,
          success: zM,
          error: KF,
        }).middleware(JF),
      )
      .add(
        bC(`threadBoundedSnapshot`, `/api/orchestration/threads/:threadId/bounded`, {
          headers: yF,
          params: aI,
          query: oI,
          success: BM,
          error: KF,
        }).middleware(JF),
      )
      .add(
        bC(`threadHistoryPage`, `/api/orchestration/threads/:threadId/history`, {
          headers: yF,
          params: aI,
          query: sI,
          success: VM,
          error: cI,
        }).middleware(JF),
      ) {},
    uI = class extends PC(`projects`)
      .add(bC(`snapshot`, `/api/projects`, { headers: vF, success: gD, error: WF }).middleware(JF))
      .add(
        xC(`mutate`, `/api/projects/mutate`, {
          headers: vF,
          payload: yD,
          success: hD,
          error: qF,
        }).middleware(JF),
      ) {},
    dI = class extends PC(`connect`)
      .add(
        xC(`linkProof`, `/api/connect/link-proof`, {
          headers: vF,
          payload: RN,
          success: UN,
          error: YF,
        }).middleware(JF),
      )
      .add(
        xC(`relayConfig`, `/api/connect/relay-config`, {
          headers: vF,
          payload: zN,
          success: XF,
          error: [...YF, RF],
        }).middleware(JF),
      )
      .add(
        bC(`linkState`, `/api/connect/link-state`, {
          headers: vF,
          success: ZF,
          error: YF,
        }).middleware(JF),
      )
      .add(
        xC(`unlink`, `/api/connect/unlink`, { headers: vF, success: XF, error: YF }).middleware(JF),
      )
      .add(
        xC(`preferences`, `/api/connect/preferences`, {
          headers: vF,
          payload: QF,
          success: ZF,
          error: YF,
        }).middleware(JF),
      )
      .add(xC(`health`, `/api/t3-connect/health`, { payload: GP, success: KP, error: YF }))
      .add(
        xC(`mintCredential`, `/api/connect/mint-credential`, {
          payload: WP,
          success: qP,
          error: YF,
        }),
      )
      .add(
        xC(`t3MintCredential`, `/api/t3-connect/mint-credential`, {
          payload: WP,
          success: qP,
          error: YF,
        }),
      ) {},
    fI = class extends PC(`pullRequests`).add(
      xC(`diff`, `/api/pull-requests/diff`, {
        headers: vF,
        payload: nE,
        success: iE,
        error: [kE, AE, EF, DF, kF],
      }).middleware(JF),
    ) {},
    pI = { params: S({ hookId: L, token: L }), success: S({ deliveryId: L }).pipe(Db(202)) },
    mI = `/api/hooks/:hookId/:token`,
    hI = class extends PC(`webhooks`)
      .add(xC(`webhookPost`, mI, pI))
      .add(SC(`webhookPut`, mI, pI))
      .add(CC(`webhookPatch`, mI, pI))
      .add(bC(`webhookGet`, mI, pI)) {};
  kC(`environment`).add(nI).add(rI).add(iI).add(lI).add(fI).add(uI).add(dI).add(hI);
  var gI = T([
      S({
        status: _(`available`),
        executablePath: y,
        source: E([`override`, `managed`, `path`]),
        version: y,
      }),
      S({ status: _(`missing`), version: y }),
      S({ status: _(`unsupported`), platform: y, arch: y, version: y }),
    ]),
    _I = E([
      `checking`,
      `waiting_for_lock`,
      `downloading`,
      `verifying`,
      `installing`,
      `validating`,
      `activating`,
    ]),
    vI = T([S({ type: _(`progress`), stage: _I }), S({ type: _(`complete`), status: gI })]),
    yI = E([
      `download_failed`,
      `invalid_checksum`,
      `install_locked`,
      `override_missing`,
      `unsupported_platform`,
      `validation_failed`,
      `write_failed`,
    ]),
    bI = class extends F()(`RelayClientInstallFailedError`, { reason: yI, message: y }) {};
  S({
    mode: _(`desktop`),
    noBrowser: x,
    port: k_,
    t3Home: g(y),
    host: y,
    desktopBootstrapToken: y,
    desktopBootstrapSecret: h(y),
    shellEnvironmentPrepared: h(x),
    tailscaleServeEnabled: x,
    tailscaleServePort: k_,
    otlpTracesUrl: g(y),
    otlpMetricsUrl: g(y),
    otlpLogsUrl: g(y),
    desktopTelemetryFd: h(z),
    desktopTelemetryControlFd: h(z),
    desktopBrowserFd: h(z),
    desktopBrowserControlFd: h(z),
    resourceMonitorPath: h(L),
  });
  var xI = { threadId: L, tabId: L };
  (T([
    S({ type: _(`attached`), ...xI }),
    S({ type: _(`detached`), ...xI }),
    S({ type: _(`cdp`), ...xI, message: y }),
  ]),
    T([
      S({ type: _(`cdp`), ...xI, message: y }),
      S({ type: _(`release`), ...xI }),
      S({ type: _(`pointer`), ...xI, phase: E([`move`, `click`]), x: Ng, y: Ng }),
    ]));
  var SI = E([`darwin`, `linux`, `win32`]);
  S({ version: _(1), requestId: L, type: _(`open-workspace`), workspaceRoot: L, platform: SI });
  var CI = E([
    `invalid-request`,
    `renderer-unavailable`,
    `environment-unavailable`,
    `platform-mismatch`,
    `project-create-failed`,
    `thread-open-failed`,
    `request-timeout`,
    `internal-error`,
  ]);
  T([
    S({ version: _(1), requestId: L, ok: _(!0), projectId: H, threadId: V }),
    S({ version: _(1), requestId: L, ok: _(!1), code: CI, message: L }),
  ]);
  var wI = E([`core`, `private-network`, `tunnel`, `manual`]),
    TI = E([`loopback`, `lan`, `private-network`, `public`]),
    EI = E([`compatible`, `mixed-content-blocked`, `requires-configuration`, `unknown`]),
    DI = E([`available`, `unavailable`, `unknown`]),
    OI = E([`desktop-core`, `desktop-addon`, `server`, `user`]);
  S({
    id: L,
    label: L,
    provider: S({ id: L, label: L, kind: wI, isAddon: x }),
    httpBaseUrl: L,
    wsBaseUrl: L,
    reachability: TI,
    compatibility: S({ hostedHttpsApp: EI, desktopApp: E([`compatible`, `unknown`]) }),
    source: OI,
    status: DI,
    isDefault: g(x),
    description: g(L),
  });
  var kI = L.check(j(64), Mg(/^[^\p{Cc}]+$/u)),
    AI = S({ id: kI, name: L.check(j(48)), kind: E([`persistent`, `incognito`]) }),
    jI = `default`,
    MI = `~effect/PrimaryKey`,
    NI = E([`ios`, `android`]),
    PI = L.check(j(128)),
    FI = S({
      id: PI.check(
        Mg(/^[a-zA-Z0-9][a-zA-Z0-9_-]*$/),
        k((e) => e !== `local` || `The local host id is reserved.`),
      ),
      label: L,
      target: L.check(Mg(/^[^\s-][^\s]*$/)),
      identityFile: g(L),
      port: g(A.check(Gg({ minimum: 1, maximum: 65535 }))),
    }),
    II = w(FI).check(
      k((e) => new Set(e.map((e) => e.id)).size === e.length || `Device host ids must be unique.`),
    ),
    LI = L.check(j(256)),
    RI = S({ hostId: PI, id: LI, platform: NI, name: L, version: y, booted: x, physical: x }),
    zI = S({ platform: NI, available: x, reason: g(y) }),
    BI = S({ requiredVersion: y, installedVersions: w(y), runningVersion: D(y) }),
    VI = S({ hub: BI, agent: BI }),
    HI = S({
      id: PI,
      kind: E([`local`, `ssh`]),
      label: L,
      platforms: w(zI),
      tools: g(VI),
      toolInspectionError: g(y),
      hubInstalled: x,
      agentDeviceInstalled: x,
    }),
    UI = E([`disabled`, `idle`, `installing`, `starting`, `ready`, `failed`]),
    WI = S({ threadId: V, hostId: PI, deviceId: LI, platform: NI, openedAt: y }),
    GI = S({
      supportsHostRetry: g(x),
      supportsToolUpdate: g(x),
      supportsToolInspection: g(x),
      hosts: w(HI),
      hostStatus: UI,
      hostStatusDetail: g(y),
      hostStatuses: C(PI, S({ status: UI, detail: g(y) })),
      devices: w(RI),
      sessions: w(WI),
      bootingDevices: g(w(S({ ...RI.fields, threadId: V }))),
      onboardingCompleted: x,
      agentAccessEnabled: x,
      hubBasePath: y,
      revision: A,
    }),
    KI = S({ updateTool: g(E([`hub`, `agent`])), inspectOnly: g(x), retryHostId: g(PI) }),
    qI = S({ enabled: g(x), agentAccessEnabled: g(x), onboardingCompleted: g(x) }),
    JI = S({ threadId: V, hostId: g(PI), deviceId: LI, platform: NI, boot: g(x) }),
    YI = S({ hostId: g(PI), threadId: V, deviceId: g(LI), shutdown: g(x) }),
    XI = S({ hostId: g(PI), deviceId: LI, platform: NI }),
    ZI = E([`light`, `dark`]),
    QI = E([`small`, `default`, `large`, `extra-large`]),
    $I = E([`none`, `grayscale`, `red-green`, `green-red`, `blue-yellow`]),
    eL = E([`portrait`, `landscape_left`, `portrait_upside_down`, `landscape_right`]),
    tL = S({
      hostId: PI,
      deviceId: LI,
      settings: S({
        appearance: g(ZI),
        textSize: g(QI),
        reduceMotion: g(x),
        increaseContrast: g(x),
        reduceTransparency: g(x),
        showBorders: g(x),
        voiceOver: g(x),
        liquidGlass: g(E([`clear`, `tinted`])),
        colorFilter: g($I),
        networkEnabled: g(x),
        location: g(D(S({ latitude: b, longitude: b }))),
      }),
      foregroundApp: D(S({ id: y, name: g(y), version: g(y) })),
      readAt: y,
    }),
    nL = E([
      `camera`,
      `microphone`,
      `photos`,
      `contacts`,
      `calendar`,
      `reminders`,
      `location`,
      `notifications`,
      `motion`,
      `media-library`,
      `faceid`,
    ]),
    rL = { hostId: g(PI), deviceId: LI },
    iL = T([
      S({ ...rL, type: _(`setAppearance`), value: ZI }),
      S({ ...rL, type: _(`setTextSize`), value: QI }),
      S({
        ...rL,
        type: _(`setToggle`),
        setting: E([
          `reduceMotion`,
          `increaseContrast`,
          `reduceTransparency`,
          `showBorders`,
          `voiceOver`,
          `networkEnabled`,
        ]),
        value: x,
      }),
      S({ ...rL, type: _(`setLiquidGlass`), value: E([`clear`, `tinted`]) }),
      S({ ...rL, type: _(`setColorFilter`), value: $I }),
      S({ ...rL, type: _(`setOrientation`), value: eL }),
      S({
        ...rL,
        type: _(`setLocation`),
        latitude: b.check(Gg({ minimum: -90, maximum: 90 })),
        longitude: b.check(Gg({ minimum: -180, maximum: 180 })),
      }),
      S({ ...rL, type: _(`clearLocation`) }),
      S({
        ...rL,
        type: _(`setPermission`),
        appId: L,
        permission: nL,
        decision: E([`grant`, `revoke`, `reset`]),
      }),
      S({ ...rL, type: _(`openUrl`), url: L }),
      S({ ...rL, type: _(`launchApp`), appId: L }),
      S({ ...rL, type: _(`terminateApp`), appId: L }),
      S({ ...rL, type: _(`shake`) }),
      S({ ...rL, type: _(`sendPush`), appId: L, payload: T([y, C(y, v)]) }),
    ]),
    aL = S(rL),
    oL = class extends F()(`DeviceHostUnavailableError`, { hostId: PI, reason: y, cause: g(M()) }) {
      get message() {
        return `Device host ${this.hostId} is unavailable: ${this.reason}`;
      }
    },
    sL = class extends F()(`DevicePlatformUnavailableError`, {
      hostId: PI,
      platform: NI,
      reason: y,
    }) {
      get message() {
        return `${this.platform} devices are unavailable on host ${this.hostId}: ${this.reason}`;
      }
    },
    cL = class extends F()(`DeviceNotFoundError`, { hostId: PI, deviceId: LI }) {
      get message() {
        return `Device ${this.deviceId} was not found on host ${this.hostId}.`;
      }
    },
    lL = class extends F()(`DeviceBootError`, {
      hostId: PI,
      deviceId: LI,
      reason: E([`disk_space`, `timeout`, `launch_failed`]),
      cause: M(),
    }) {
      get message() {
        let e = {
          disk_space: `There is not enough free disk space on the environment server.`,
          timeout: `The device did not become ready in time.`,
          launch_failed: `The simulator or emulator could not start. Check its configuration on the environment server.`,
        }[this.reason];
        return `Device ${this.deviceId} failed to boot: ${e}`;
      }
    },
    uL = class extends F()(`DeviceOperationError`, {
      operation: y,
      reason: E([
        `command_failed`,
        `request_failed`,
        `invalid_payload`,
        `settings_failed`,
        `hub_rejected`,
      ]),
      exitCode: g(b),
      cause: M(),
    }) {
      get message() {
        let e = {
          command_failed: `The device command failed${this.exitCode === void 0 ? `` : ` (exit code ${this.exitCode})`}.`,
          request_failed: `Could not communicate with device support. Try refreshing devices.`,
          invalid_payload: `The device request could not be encoded.`,
          settings_failed: `Could not read or save device settings.`,
          hub_rejected: `The device hub could not complete the request.`,
        }[this.reason];
        return `Device ${this.operation} failed: ${e}`;
      }
    },
    dL = class extends F()(`DeviceActionUnavailableError`, {
      operation: y,
      platform: NI,
      reason: E([`unsupported`, `helper_missing`]),
    }) {
      get message() {
        return this.reason === `helper_missing`
          ? `Device ${this.operation} requires a helper missing from this install. Set up device support again.`
          : `Device ${this.operation} is not supported on ${this.platform}.`;
      }
    },
    fL = T([oL, sL, cL, lL, uL, dL]);
  (S({
    hostStatuses: GI.fields.hostStatuses,
    hosts: w(HI),
    devices: w(RI),
    open: w(S({ hostId: PI, deviceId: LI })),
  }),
    S({
      deviceId: g(
        LI.annotate({
          description: `Simulator udid or emulator serial from device_list. Omit to use the booted device for the platform, or the most recently used one.`,
        }),
      ),
      platform: g(
        NI.annotate({
          description: `Required when deviceId is omitted and both platforms are available.`,
        }),
      ),
      hostId: g(PI.annotate({ description: `Device host from device_list. Defaults to local.` })),
    }).annotate({
      description: `Boots the device if needed, starts its live stream, and opens the Device panel so the user can watch. Returns how to drive it with the agent-device CLI.`,
    }),
    S({ device: RI, agentDevice: S({ command: y, targetArgs: w(y) }), quickStart: y }),
    S({
      deviceId: g(
        LI.annotate({
          description: `Device from device_list. Omit to use the device most recently opened in this thread.`,
        }),
      ),
      hostId: g(PI),
    }),
    S({ device: RI, screenshot: S({ mimeType: _(`image/png`), data: y, width: A, height: A }) }),
    S({
      deviceId: g(
        LI.annotate({ description: `Device to close. Omit to close every device in this thread.` }),
      ),
      hostId: g(PI),
      shutdown: g(
        x.annotate({ description: `Also power the simulator or emulator off. Defaults to false.` }),
      ),
    }),
    T([
      class extends F()(`DeviceToolUnavailableError`, { reason: y, cause: g(M()) }) {
        get message() {
          return this.reason;
        }
      },
      oL,
      sL,
      cL,
      lL,
      uL,
      dL,
    ]));
  var pL = 256,
    mL = [
      `thread.jump.1`,
      `thread.jump.2`,
      `thread.jump.3`,
      `thread.jump.4`,
      `thread.jump.5`,
      `thread.jump.6`,
      `thread.jump.7`,
      `thread.jump.8`,
      `thread.jump.9`,
    ],
    hL = [
      `modelPicker.jump.1`,
      `modelPicker.jump.2`,
      `modelPicker.jump.3`,
      `modelPicker.jump.4`,
      `modelPicker.jump.5`,
      `modelPicker.jump.6`,
      `modelPicker.jump.7`,
      `modelPicker.jump.8`,
      `modelPicker.jump.9`,
    ],
    gL = [
      `thread.stop`,
      `thread.steerQueuedMessage`,
      `thread.editQueuedMessage`,
      `thread.previous`,
      `thread.next`,
      `thread.copyReference`,
      `thread.settle`,
      `thread.pin`,
      `thread.undo`,
      ...mL,
    ],
    _L = [
      `sidebar.toggle`,
      `navigation.back`,
      `navigation.forward`,
      `terminal.toggle`,
      `terminal.split`,
      `terminal.splitVertical`,
      `terminal.new`,
      `terminal.close`,
      `rightPanel.toggle`,
      `rightPanel.new`,
      `threadPanel.toggle`,
      `rightPanel.toggleMaximized`,
      `rightPanel.close`,
      `view.reopenClosed`,
      `pullRequest.copyNumber`,
      `diff.toggle`,
      `preview.toggle`,
      `preview.refresh`,
      `preview.focusUrl`,
      `preview.zoomIn`,
      `preview.zoomOut`,
      `preview.resetZoom`,
      `commandPalette.toggle`,
      `filePicker.toggle`,
      `projectSearch.toggle`,
      `usage.open`,
      `theme.select`,
      `appearance.cycle`,
      `themeEditor.toggle`,
      `composer.stash`,
      `composer.sendAlternate`,
      `composer.sendBackground`,
      `composer.sendAndNewThread`,
      `composer.host`,
      `composer.cycleHost`,
      `composer.effort`,
      `composer.mode`,
      `composer.workspace`,
      `composer.previousWorktree`,
      `composer.branch`,
      `chat.new`,
      `chat.newLocal`,
      `chat.newWithoutProject`,
      `chat.find`,
      `editor.openFavorite`,
      `usage.cost`,
      `usage.tokens`,
      `usage.limits`,
      `usage.period.day`,
      `usage.period.week`,
      `usage.period.month`,
      `usage.period.quarter`,
      `modelPicker.toggle`,
      `modelPicker.previousProvider`,
      `modelPicker.nextProvider`,
      ...hL,
      ...gL,
    ],
    vL = ng([_(`script.`), Qg.check(j(24), Mg(/^[a-z0-9][a-z0-9-]*$/)), _(`.run`)]),
    yL = T([E(_L), vL]),
    bL = I.check(qg(1), j(64)),
    xL = I.check(qg(1), j(pL));
  w(S({ key: bL, command: yL, when: g(xL) })).check(j(256));
  var SL = S({ key: bL, metaKey: x, ctrlKey: x, shiftKey: x, altKey: x, modKey: x });
  S({ command: yL, shortcut: SL });
  var CL = fg(() => wL),
    wL = T([
      S({ type: _(`identifier`), name: Qg }),
      S({ type: _(`not`), node: CL }),
      S({ type: _(`and`), left: CL, right: CL }),
      S({ type: _(`or`), left: CL, right: CL }),
    ]),
    TL = P_(
      S({ command: yL, shortcut: SL, whenAst: g(wL) }).annotate({
        parseOptions: { onExcessProperty: `ignore` },
      }),
    ).check(j(256)),
    EL = class extends F()(`KeybindingsConfigParseError`, {
      configPath: y,
      detail: y,
      cause: g(M()),
    }) {
      get message() {
        return `Unable to parse keybindings config at ${this.configPath}: ${this.detail}`;
      }
    },
    DL = L.check(j(2048)),
    OL = w(DL).check(j(32)),
    kL = y.check(j(512)),
    AL = L.check(j(128)),
    jL = 3840,
    ML = 8294400,
    NL = A.check(Gg({ minimum: 240, maximum: jL })),
    PL = k(
      ({ width: e, height: t }) =>
        e * t <= 8294400 || `Viewport area must not exceed 8294400 pixels.`,
    ),
    FL = S({ width: NL, height: NL }).check(PL),
    IL = S({ width: A.check(Vg(0)), height: A.check(Vg(0)) }),
    LL = [
      `iphone-se`,
      `iphone-xr`,
      `iphone-12-pro`,
      `iphone-14-pro-max`,
      `pixel-7`,
      `samsung-galaxy-s8-plus`,
      `samsung-galaxy-s20-ultra`,
      `ipad-mini`,
      `ipad-air`,
      `ipad-pro`,
      `surface-pro-7`,
      `surface-duo`,
      `galaxy-z-fold-5`,
      `asus-zenbook-fold`,
      `samsung-galaxy-a51-71`,
      `nest-hub`,
      `nest-hub-max`,
    ],
    RL = E(LL),
    zL = [
      `desktop-1920x1080`,
      `desktop-1440x900`,
      `laptop-1366x768`,
      `laptop-1280x800`,
      `ipad-pro-11`,
      `iphone-15-pro`,
      `pixel-8`,
      `galaxy-s24`,
    ],
    BL = E([...LL, ...zL]),
    VL = T([
      Dg(`fill`, {}),
      Dg(`freeform`, { ...FL.fields }).check(PL),
      Dg(`preset`, { ...FL.fields, presetId: BL }).check(PL),
    ]),
    HL = { _tag: `fill` },
    UL = E([0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4, 5]),
    WL = E([`system`, `light`, `dark`]),
    GL = `system`,
    KL = S({ id: y, fileName: y }),
    qL = T([
      Dg(`Idle`, {}),
      Dg(`Loading`, { url: DL, title: kL }),
      Dg(`Success`, { url: DL, title: kL }),
      Dg(`LoadFailed`, { url: DL, title: kL, code: A, description: y, download: g(KL) }),
    ]),
    JL = E([`desktop`, `server`]);
  S({ need: E([`sandbox`, `libraries`]), command: y });
  var YL = S({
      threadId: L,
      tabId: AL,
      navStatus: qL,
      canGoBack: x,
      canGoForward: x,
      viewport: g(VL),
      colorScheme: g(WL),
      zoomFactor: g(UL),
      profileId: g(kI),
      runtime: g(JL),
      automationOwner: g(y),
      reveal: g(x),
      revealRequest: g(S({ id: y, force: x })),
      updatedAt: y,
    }),
    XL = S({
      threadId: V,
      url: g(DL),
      viewport: g(VL),
      profileId: g(kI),
      runtime: g(JL),
      reveal: g(x),
    }),
    ZL = S({ threadId: V, tabId: AL, url: DL, resolvedTitle: g(kL) }),
    QL = S({ threadId: V, tabId: AL, navStatus: qL, canGoBack: x, canGoForward: x }),
    $L = S({ threadId: V, tabId: AL }),
    eR = S({ threadId: V, tabId: AL, viewport: VL }),
    tR = S({
      threadId: V,
      tabId: AL,
      colorScheme: g(WL),
      zoomFactor: g(UL),
      hardReload: g(x),
      clear: g(E([`cookies`, `cache`])),
    }),
    nR = S({ threadId: V, tabId: g(AL) }),
    rR = S({ profiles: w(AI).check(j(24)), defaultProfileId: kI }),
    iR = S({ profileId: kI }),
    aR = S({ threadId: V }),
    oR = S({ sessions: w(YL), serverEpoch: L, revision: R }),
    sR = S({ threadId: L, tabId: AL, createdAt: y, serverEpoch: L, revision: z }),
    cR = T([
      S({ ...sR.fields, type: _(`opened`), snapshot: YL }),
      S({ ...sR.fields, type: _(`navigated`), snapshot: YL }),
      S({
        ...sR.fields,
        type: _(`resized`),
        snapshot: YL,
        request: g(S({ hardReload: g(x), clear: g(E([`cookies`, `cache`])) })),
      }),
      S({
        ...sR.fields,
        type: _(`failed`),
        url: DL,
        title: kL,
        code: A,
        description: y,
        download: g(KL),
      }),
      S({ ...sR.fields, type: _(`closed`) }),
    ]),
    lR = S({
      servers: w(
        S({
          host: L,
          port: A.check(Vg(0)).check(Ug(65536)),
          url: DL,
          processName: D(L),
          pid: D(A.check(Vg(0))),
          terminal: D(S({ threadId: V, terminalId: L })),
        }),
      ),
      scannedAt: y,
      configuredUrlProbing: g(_(!0)),
    }),
    uR = class extends F()(`PreviewSessionLookupError`, { threadId: y, tabId: y }) {
      get message() {
        return `Unknown preview session: thread=${this.threadId}, tab=${this.tabId}`;
      }
    },
    dR = class extends F()(`PreviewInvalidUrlError`, {
      inputLength: b,
      reason: E([`empty`, `parse`, `unsupported-protocol`, `unexpected`]),
      protocol: g(y),
      cause: M(),
    }) {
      get message() {
        let e = this.protocol === void 0 ? `` : `: ${this.protocol}`;
        return `Invalid preview URL (${this.reason}${e}; input length ${this.inputLength}).`;
      }
    },
    fR = class extends F()(`PreviewControlRequiredError`, { tabId: y }) {
      get message() {
        return `Take control of the server browser and use its viewer controls.`;
      }
    },
    pR = class extends F()(`PreviewClearProfileError`, { profileId: y, cause: M() }) {
      get message() {
        return `The environment could not delete this browser profile's data.`;
      }
    },
    mR = T([uR, dR, fR]),
    hR = E([`claude`, `codex`, `grok`, `cursor`, `opencode`, `antigravity`, `pi`]),
    gR = L.check(Mg(/^[^/]+\/.+$/)).pipe(mg(`UsageCatalogModelId`)),
    _R = S({ provider: hR, apiProvider: I, model: L, catalogModelId: gR }),
    vR = L.check(Mg(/^\d{4}-\d{2}-\d{2}$/)).pipe(mg(`UsageDay`)),
    yR = E([`day`, `hour`]),
    bR = E([`providerReported`, `modelPriced`, `userTagged`, `unpriced`]),
    xR = S({
      uncachedInputTokens: R,
      cachedInputTokens: R,
      cacheCreationTokens: R,
      outputTokens: R,
      reasoningTokens: R,
    }),
    SR = S({ input: b, cacheRead: b, cacheWrite: b, output: b }),
    CR = S({
      day: vR,
      hourStart: g(L),
      provider: hR,
      model: L,
      apiProvider: y.pipe(O(m(``))),
      sourcePath: g(L),
      totals: xR,
      costUsd: b,
      cacheSavingsUsd: b,
      inputTokensEstimated: x.pipe(O(m(!1))),
      categoryCostUsd: g(SR),
      fastCostUsd: g(b),
      ultrafastCostUsd: g(b),
      speedPremiumUsd: g(b),
      costSource: bR,
      pricedAs: D(gR).pipe(O(m(null))),
      records: R,
      unpricedRecords: R,
      sessions: R,
    }),
    wR = S({
      fingerprint: S({ hostId: L, provider: hR, resolvedHomePath: L, volumeId: y }),
      status: E([`ok`, `missing`, `partial`, `failed`]),
      scannedFiles: R,
      skippedFiles: R,
      malformedRecords: R,
      distinctSessions: R,
      message: D(L),
      action: h(_(`enableCursorKeychain`)),
      refreshing: h(_(!0)),
    }),
    TR = S({
      status: E([`fresh`, `cached`, `unavailable`]),
      source: L,
      fetchedAt: D(y),
      knownModels: R,
    }),
    ER = S({
      sinceDay: vR,
      untilDay: vR,
      timeZone: L,
      modelAliases: w(_R).pipe(O(m([]))),
      resolution: g(yR),
      sinceTime: g(L),
      untilTime: g(L),
      awaitRefresh: g(x),
    }),
    DR = S({
      id: gR,
      providerName: L,
      modelName: L,
      inputCostPerMillion: b,
      outputCostPerMillion: b,
    }),
    OR = S({ query: L }),
    kR = S({ models: w(DR) }),
    AR = S({
      contractVersion: b,
      readAt: y,
      timeZone: L,
      sinceDay: vR,
      untilDay: vR,
      buckets: P_(CR),
      sources: P_(wR),
      pricing: TR,
      scanDurationMs: R,
    }),
    jR = class extends F()(`UsageReadError`, {
      reason: E([`scanFailed`, `invalidWindow`]),
      detail: L,
      cause: g(M()),
    }) {
      get message() {
        return `Usage read failed (${this.reason}): ${this.detail}`;
      }
    },
    MR = E([`locale`, `12-hour`, `24-hour`]),
    NR = `locale`,
    PR = E([`stacked`, `split`]),
    FR = `stacked`,
    IR = E([`updated_at`, `created_at`, `manual`]),
    LR = `updated_at`,
    RR = E([`updated_at`, `created_at`]),
    zR = `updated_at`,
    BR = E([`repository`, `repository_path`, `separate`]),
    VR = `repository`,
    HR = A.check(Gg({ minimum: 1, maximum: 15 })),
    UR = 6,
    WR = b.check(Gg({ minimum: 1, maximum: 90 })),
    GR = 3,
    KR = A.check(Gg({ minimum: 40, maximum: 100 })),
    qR = 80,
    JR = A.check(Gg({ minimum: 50, maximum: 200 })),
    YR = 100,
    XR = A.check(Gg({ minimum: 0, maximum: 400 })),
    ZR = 0,
    QR = A.check(Gg({ minimum: 12, maximum: 20 })),
    $R = A.check(Gg({ minimum: 12, maximum: 20 })),
    ez = A.check(Gg({ minimum: 10, maximum: 18 })),
    tz = A.check(Gg({ minimum: 8, maximum: 20 })),
    nz = 12,
    rz = E([`artwork`, `pill`, `none`]),
    iz = `artwork`,
    az = SL.check(
      k(
        (e) =>
          e.metaKey ||
          e.ctrlKey ||
          e.shiftKey ||
          e.altKey ||
          e.modKey ||
          `Snapshot shortcut requires a modifier.`,
      ),
    ),
    oz = E([`shift`, `meta`, `control`, `alt`]),
    sz = T([S({ kind: _(`both-shift-keys`) }), S({ kind: _(`modifier-pair`), modifier: oz }), az]),
    cz = E([`soft-pop`, `camera-shutter`]),
    lz = `soft-pop`,
    uz = { kind: `both-shift-keys` },
    dz = E([`off`, `notifications`, `sound`, `notifications-and-sound`]),
    fz = E([`direct`, `hold`, `double-click`]),
    pz = `hold`,
    mz = T([
      fz,
      x.pipe(xg(fz, Ed({ decode: (e) => (e ? `hold` : `direct`), encode: (e) => e === `hold` }))),
    ]),
    hz = y.check(j(200)),
    gz = y.check(j(64)),
    _z = HL,
    vz = E([30, 60]),
    yz = E([`system`, `app`]),
    bz = `system`,
    xz = C(L, A.check(Gg({ minimum: 0, maximum: 100 }))),
    Sz = E([`red-green`, `blue-orange`]),
    Cz = E([`comfortable`, `wide`, `full`]),
    wz = S({ provider: q, model: L }),
    Tz = w(wz),
    Ez = C(q, S({ hiddenModels: w(y).pipe(O(m([]))), modelOrder: w(y).pipe(O(m([]))) })),
    Dz = Xh(
      S({
        notificationMode: dz.pipe(O(m(`off`))),
        inAppNotificationsEnabled: x.pipe(O(m(!1))),
        diffColorScheme: Sz.pipe(O(m(`red-green`))),
        chatWidth: Cz.pipe(O(m(`comfortable`))),
        loadBalancingEnabled: x.pipe(O(m(!1))),
        loadBalancingWeights: xz.pipe(O(m({}))),
        appearanceContrast: JR.pipe(O(m(YR))),
        panelAnimationDurationMs: XR.pipe(O(m(ZR))),
        browserDefaultViewport: VL.pipe(O(m(_z))),
        browserDefaultZoomFactor: UL.pipe(O(m(1))),
        browserDefaultAppearance: WL.pipe(O(m(GL))),
        browserRecordingFrameRate: vz.pipe(O(m(30))),
        browserRecordingShowKeyPresses: x.pipe(O(m(!1))),
        browserRecordingShowMousePresses: x.pipe(O(m(!1))),
        browserLinkTarget: yz.pipe(O(m(bz))),
        browserAutoShowFloatingPreview: x.pipe(O(m(!0))),
        browserProfiles: w(AI).pipe(O(m([]))),
        browserDefaultProfileId: kI.pipe(O(m(jI))),
        confirmQuit: mz.pipe(O(m(pz))),
        confirmThreadArchive: x.pipe(O(m(!1))),
        confirmThreadDelete: x.pipe(O(m(!0))),
        confirmThreadUnpin: x.pipe(O(m(!1))),
        usageModelAliases: w(_R).pipe(O(m([]))),
        dismissedProviderUpdateNotificationKeys: w(L).pipe(O(m([]))),
        diffFilesCollapsed: x.pipe(O(m(!0))),
        diffIgnoreWhitespace: x.pipe(O(m(!0))),
        diffLayout: PR.pipe(O(m(FR))),
        environmentIdentificationMode: rz.pipe(O(m(iz))),
        glassOpacity: KR.pipe(O(m(qR))),
        fontSizeInterface: QR.pipe(O(m(16))),
        fontSizePrompt: $R.pipe(O(m(14))),
        fontSizeCode: ez.pipe(O(m(13))),
        fontSizeTerminal: tz.pipe(O(m(nz))),
        fontFamilyCode: hz.pipe(O(m(``))),
        fontFamilyComposer: hz.pipe(O(m(``))),
        fontFamilySans: hz.pipe(O(m(``))),
        fontFamilyTerminal: hz.pipe(O(m(``))),
        fontSmoothing: x.pipe(O(m(!0))),
        onboardingCompletedAt: D(y).pipe(O(m(null))),
        persistComposerContextStrip: x.pipe(O(m(!1))),
        favorites: Tz.pipe(O(m([]))),
        providerModelPreferences: Ez.pipe(O(m({}))),
        pullRequestMergeMethodOverrides: C(L, cT).pipe(O(m({}))),
        planModeEnabled: x.pipe(O(m(!1))),
        contextWindowMeterEnabled: x.pipe(O(m(!1))),
        composerCollapseOnScroll: x.pipe(O(m(!0))),
        composerRichTextEnabled: x.pipe(O(m(!0))),
        sendShortcut: E([`enter`, `mod-enter-multiline`, `mod-enter`]).pipe(O(m(`enter`))),
        followUpBehavior: E([`queue`, `steer`]).pipe(O(m(`queue`))),
        proactivePanelsEnabled: x.pipe(O(m(!1))),
        showSkillsInSlashMenu: x.pipe(O(m(!0))),
        legacySidebarEnabled: x.pipe(O(m(!1))),
        sidebarWorkingShelfEnabled: x.pipe(O(m(!1))),
        sidebarProjectGroupingMode: BR.pipe(O(m(VR))),
        sidebarProjectGroupingOverrides: C(L, BR).pipe(O(m({}))),
        sidebarProjectSortOrder: IR.pipe(O(m(LR))),
        sidebarThreadSortOrder: RR.pipe(O(m(zR))),
        sidebarThreadPreviewCount: HR.pipe(O(m(UR))),
        timestampFormat: MR.pipe(O(m(NR))),
        snapShotEnabled: x.pipe(O(m(!1))),
        snapShotIncludeAccessibility: x.pipe(O(m(!0))),
        snapShotShortcut: sz.pipe(O(m(uz))),
        snapShotPlaySound: x.pipe(O(m(!0))),
        snapShotSound: cz.pipe(O(m(lz))),
        snapShotFlash: x.pipe(O(m(!0))),
        snapShotAnimations: x.pipe(O(m(!0))),
        wordWrap: x.pipe(O(m(!0))),
      }),
    )({}),
    Oz = b.check(Pg(), Hg(0)),
    kz = S({
      inputCostPerMillionTokens: Oz,
      outputCostPerMillionTokens: Oz,
      cacheReadCostPerMillionTokens: h(Oz),
      cacheWriteCostPerMillionTokens: h(Oz),
    }),
    Az = (e) => I.pipe(xg(y, Td({ decode: (t) => m(t || e), encode: (e) => m(e) })), O(m(e)));
  function jz(e, t) {
    return S(e).pipe(
      Rh({ providerSettingsFormSchema: t?.order === void 0 ? void 0 : { order: t.order } }),
    );
  }
  (jz(
    {
      setupMode: h(E([`managed`, `existing`])).pipe(zh({ providerSettingsForm: { hidden: !0 } })),
      enabled: x.pipe(O(m(!0)), zh({ providerSettingsForm: { hidden: !0 } })),
      binaryPath: Az(`codex`).pipe(
        zh({
          title: `Binary path`,
          description: `Path to the Codex binary used by this instance.`,
          providerSettingsForm: { placeholder: `codex`, clearWhenEmpty: `omit` },
        }),
      ),
      homePath: I.pipe(
        O(m(``)),
        zh({
          title: `CODEX_HOME path`,
          description: `Custom Codex home and config directory.`,
          providerSettingsForm: { placeholder: `~/.codex`, clearWhenEmpty: `omit` },
        }),
      ),
      shadowHomePath: I.pipe(
        O(m(``)),
        zh({
          title: `Shadow home path`,
          description: `Account-specific Codex home. Keeps auth.json separate while sharing state from CODEX_HOME.`,
          providerSettingsForm: { placeholder: `~/.codex-t3/personal`, clearWhenEmpty: `omit` },
        }),
      ),
      launchArgs: I.pipe(
        O(m(``)),
        zh({
          title: `Launch arguments`,
          description: `Additional CLI arguments passed to codex app-server on session start.`,
        }),
      ),
      customModels: w(wy).pipe(O(m([])), zh({ providerSettingsForm: { hidden: !0 } })),
    },
    { order: [`binaryPath`, `homePath`, `shadowHomePath`, `launchArgs`] },
  ),
    jz(
      {
        enabled: x.pipe(O(m(!0)), zh({ providerSettingsForm: { hidden: !0 } })),
        binaryPath: Az(`claude`).pipe(
          zh({
            title: `Binary path`,
            description: `Path to the Claude binary used by this instance.`,
            providerSettingsForm: { placeholder: `claude`, clearWhenEmpty: `omit` },
          }),
        ),
        homePath: I.pipe(
          O(m(``)),
          zh({
            title: `CLAUDE_CONFIG_DIR path`,
            description: `Custom Claude home and config directory. Keeps .claude.json and .claude separate.`,
            providerSettingsForm: { placeholder: `~/.claude`, clearWhenEmpty: `omit` },
          }),
        ),
        customModels: w(wy).pipe(O(m([])), zh({ providerSettingsForm: { hidden: !0 } })),
        launchArgs: y.pipe(
          O(m(``)),
          zh({
            title: `Launch arguments`,
            description: `Additional CLI arguments passed on session start.`,
            providerSettingsForm: { placeholder: `e.g. --chrome`, clearWhenEmpty: `omit` },
          }),
        ),
        autoCompactWindow: I.check(Mg(/^(?:|[1-9]\d{5}|1000000)$/)).pipe(
          O(m(``)),
          zh({
            title: `Auto-compact after`,
            description: `Compact after 100,000 to 1,000,000 tokens. Leave empty to use Claude's default.`,
            providerSettingsForm: { placeholder: `e.g. 300000`, clearWhenEmpty: `omit` },
          }),
        ),
      },
      { order: [`binaryPath`, `homePath`, `autoCompactWindow`, `launchArgs`] },
    ));
  var Mz = [
      { value: `oauth-personal`, label: `Google account` },
      { value: `oauth-business`, label: `Gemini Enterprise` },
      { value: `gemini-api-key`, label: `Gemini API key` },
      { value: `agent-platform`, label: `Agent Platform (Vertex AI)` },
    ],
    Nz = E(Mz.map((e) => e.value));
  jz(
    {
      enabled: x.pipe(O(m(!1)), zh({ providerSettingsForm: { hidden: !0 } })),
      authMethod: Nz.pipe(
        O(m(`oauth-personal`)),
        zh({
          title: `Sign-in method`,
          description: `Google accounts use your subscription; API keys and Agent Platform bill usage.`,
          providerSettingsForm: { control: `select`, options: Mz, clearWhenEmpty: `omit` },
        }),
      ),
      apiKey: I.pipe(
        O(m(``)),
        zh({
          title: `API key`,
          description: `Gemini or Vertex AI express key. Stored in plain text.`,
          providerSettingsForm: {
            control: `password`,
            placeholder: `Optional`,
            clearWhenEmpty: `omit`,
          },
        }),
      ),
      gcpProject: I.pipe(
        O(m(``)),
        zh({
          title: `GCP project`,
          description: `Required for Gemini Enterprise. Agent Platform uses it when no API key is set.`,
          providerSettingsForm: { placeholder: `my-project-id`, clearWhenEmpty: `omit` },
        }),
      ),
      gcpLocation: I.pipe(
        O(m(``)),
        zh({
          title: `GCP location`,
          description: `Region for Gemini Enterprise or Agent Platform.`,
          providerSettingsForm: { placeholder: `us-central1`, clearWhenEmpty: `omit` },
        }),
      ),
      binaryPath: I.pipe(
        O(m(``)),
        zh({
          title: `Binary path`,
          description: `Custom ACP executable. Leave empty to select automatically.`,
          providerSettingsForm: { placeholder: `Automatic`, clearWhenEmpty: `persist` },
        }),
      ),
      customModels: w(wy).pipe(O(m([])), zh({ providerSettingsForm: { hidden: !0 } })),
    },
    { order: [`authMethod`, `apiKey`, `gcpProject`, `gcpLocation`, `binaryPath`] },
  );
  var Pz = S({
    kind: _(`cliproxy`),
    label: g(L),
    url: L,
    managementKey: I.pipe(O(m(``))),
    enabled: x.pipe(O(m(!0))),
  });
  L.pipe(xg(y, kd()));
  var Fz = C(y, v),
    Iz = S({
      otlpTracesUrl: I.pipe(O(m(``))),
      otlpMetricsUrl: I.pipe(O(m(``))),
      otlpLogsUrl: I.pipe(O(m(``))),
    }),
    Lz = E([`repo_conventions`, `conventional_commits`, `custom`]),
    Rz = S({
      mode: Lz.pipe(O(m(`repo_conventions`))),
      customInstructions: I.pipe(O(m(``))),
      followChangeRequestTemplates: x.pipe(O(m(!0))),
    }),
    zz = E([`static`, `semantic`, `custom`]),
    Bz = mi(30),
    Vz = hi(5),
    Hz = E([`balanced`, `performance`, `battery-saver`]),
    Uz = `balanced`,
    Wz = E([`balanced`, `performance`, `battery-saver`, `custom`]),
    Gz = S({
      automaticGitFetchInterval: h(p_),
      providerHealthRefreshInterval: h(p_),
      hostPowerMonitorActiveInterval: h(p_),
      hostPowerMonitorIdleInterval: h(p_),
      idleClientTtl: h(p_),
      pauseWhenHostLocked: h(x),
      pauseWhenHostLowPower: h(x),
      pauseWhenClientLowPower: h(x),
      pauseWhenOnBattery: h(x),
    }),
    Kz = S({
      schemaVersion: _(1).pipe(O(m(1))),
      profile: Wz.pipe(O(m(Uz))),
      baseProfile: h(Hz),
      overrides: Gz.pipe(O(m({}))),
    }).pipe(O(m({}))),
    qz = E([`turn`, `paragraph`]),
    Jz = D(A.check(Gg({ minimum: 1, maximum: 3650 }))),
    Yz = E([`any-local-files`, `uncommitted-changes`, `tracked-changes`]),
    Xz = S({
      worktreeKeepWhen: Yz.pipe(O(m(`uncommitted-changes`))),
      worktreeAfterDays: Jz,
      worktreeOnMerge: x,
      worktreeOnDelete: x,
      worktreeUnchanged: x,
    }),
    Zz = D(T([S({ mode: _(`off`) }), S({ mode: _(`custom`), rules: Xz })])),
    Qz = S({
      worktreeCleanup: h(Zz),
      defaultModelSelection: h(D(bw)),
      defaultRuntimeMode: h(VE),
      defaultThreadEnvMode: h(aS),
      newWorktreesStartFromOrigin: h(x),
      worktreeSubmodules: j_(oS),
      defaultAutoPull: h(x),
      defaultProjectScripts: h(w(tD)),
      enableAgentBrowserAccess: h(x),
      enableAgentDeviceAccess: h(x),
      textGenerationModelSelection: h(bw),
      sourceControlWriterModelSelection: h(D(bw)),
      reviewGuideModelSelection: h(D(bw)),
      reviewGuideInstructions: h(I),
      sourceControlWritingStyle: h(Rz),
      removeAgentCreditsOnMerge: h(x),
      branchNamingMode: h(zz),
      branchNamePrefix: h(I),
      branchNameInstructions: h(I),
      pullRequestMergeMethod: h(D(cT)),
      sidebarAutoSettleOnMerge: h(x),
      sidebarAutoSettleAfterDays: h(D(WR)),
      continueThreadsAfterServerUpdate: h(x),
      responseStreamingMode: h(qz),
    }),
    $z = S({
      kind: E([`worktree`, `browser-artifacts`, `logs`]),
      outcome: E([`removed`, `kept`, `failed`]),
      reason: y,
      path: D(y),
      threadId: D(V),
      threadTitle: D(y),
      bytes: D(b).pipe(O(m(null))),
      files: D(b).pipe(O(m(null))),
    }),
    eB = S({
      trigger: E([`automatic`, `manual`]),
      startedAt: y,
      finishedAt: y,
      entries: w($z),
      counts: S({ removed: b, kept: b, failed: b }),
      omittedCount: b,
      bytesFreed: b.pipe(O(m(0))),
    }),
    tB = S({
      worktreeKeepWhen: Yz.pipe(O(m(`uncommitted-changes`))),
      worktreeAfterDays: Jz.pipe(O(m(null))),
      worktreeOnMerge: x.pipe(O(m(!1))),
      worktreeOnDelete: x.pipe(O(m(!1))),
      worktreeUnchanged: x.pipe(O(m(!1))),
      browserArtifactsAfterDays: Jz.pipe(O(m(null))),
      logsAfterDays: Jz.pipe(O(m(null))),
    }),
    nB = S({
      favorites: D(Tz).pipe(O(m(null))),
      providerModelPreferences: D(Ez).pipe(O(m(null))),
      worktreeCleanup: Zz.pipe(O(m(null))),
      storageCleanup: tB.pipe(O(m(Xh(tB)({})))),
      worktreesDirectory: I.pipe(O(m(``))),
      previousWorktreesDirectories: w(I).pipe(O(m([]))),
      responseStreamingMode: qz.pipe(O(m(`paragraph`))),
      enableProviderUpdateChecks: x.pipe(O(m(!0))),
      continueThreadsAfterServerUpdate: x.pipe(O(m(!1))),
      enableAgentBrowserAccess: x.pipe(O(m(!0))),
      projectAgentBrowserAccessOverrides: C(H, x).pipe(O(m({}))),
      defaultAutoPull: x.pipe(O(m(!1))),
      defaultProjectScripts: w(tD).pipe(O(m([]))),
      projectScriptOverrides: C(H, D(w(tD))).pipe(O(m({}))),
      projectAutoPullOverrides: C(H, x).pipe(O(m({}))),
      defaultModelSelection: D(bw).pipe(O(m(null))),
      defaultRuntimeMode: VE.pipe(O(m(HE))),
      projectSettingsOverrides: C(H, Qz).pipe(O(m({}))),
      projectSettingsFolded: x.pipe(O(m(!1))),
      enableAgentDeviceAccess: x.pipe(O(m(!1))),
      enableDeviceSupport: x.pipe(O(m(!1))),
      deviceOnboardingCompleted: x.pipe(O(m(!1))),
      deviceHosts: II.pipe(O(m([]))),
      sidebarAutoSettleAfterDays: D(WR).pipe(O(m(GR))),
      snoozeLimitedThreads: x.pipe(O(m(!1))),
      autoResumeLimitedThreads: x.pipe(O(m(!1))),
      sidebarAutoSettleOnMerge: x.pipe(O(m(!0))),
      backgroundActivity: Kz,
      automaticGitFetchInterval: p_.pipe(O(m(yi(Bz)))),
      providerHealthRefreshInterval: p_.pipe(O(m(yi(Vz)))),
      backgroundActivityProfile: Hz.pipe(O(m(Uz))),
      defaultTheme: gz.pipe(O(m(``))),
      defaultThemeSetAt: y.check(j(64)).pipe(O(m(``))),
      environmentIcon: M_(rS).pipe(O(m(null))),
      defaultThreadEnvMode: N_(aS),
      newWorktreesStartFromOrigin: x.pipe(O(m(!0))),
      worktreeSubmodules: M_(oS).pipe(O(m(null))),
      addProjectBaseDirectory: I.pipe(O(m(``))),
      textGenerationModelSelection: bw.pipe(
        O(
          m({
            instanceId: q.make(`codex`),
            model: Ty,
            options: [{ id: `reasoningEffort`, value: `low` }],
          }),
        ),
      ),
      branchNamingMode: zz.pipe(O(m(`static`))),
      branchNamePrefix: I.pipe(O(m(`t3`))),
      branchNameInstructions: I.pipe(O(m(``))),
      removeAgentCreditsOnMerge: x.pipe(O(m(!1))),
      sourceControlWritingStyle: Rz.pipe(O(m({}))),
      sourceControlWriterModelSelection: D(bw).pipe(O(m(null))),
      reviewGuidesEnabled: x.pipe(O(m(!1))),
      reviewGuideModelSelection: D(bw).pipe(O(m(null))),
      reviewGuideInstructions: I.pipe(O(m(``))),
      pullRequestMergeMethod: D(cT).pipe(O(m(null))),
      providerInstances: C(q, iy).pipe(O(m({}))),
      observability: Iz.pipe(O(m({}))),
      sourceControlHosts: C(y, Fz).pipe(O(m({}))),
      usageLimitSources: C(KD, Pz).pipe(O(m({}))),
      cursorKeychainUsageEnabled: x.pipe(O(m(!1))),
      usagePriceOverrides: C(L, kz).pipe(O(m({}))),
      usageModelMappings: C(L, L).pipe(O(m({}))),
    }),
    rB = Xh(nB)({});
  (new Set([`cursor`, `grok`, `muse`, `pi`, `opencode`, `antigravity`].map((e) => ny.make(e))),
    new Set([`codex`, `claudeAgent`].map((e) => q.make(e))));
  var iB = E([
      `normalize`,
      `check-exists`,
      `create-provider-instance`,
      `read-file`,
      `read-provider-history`,
      `read-project-settings`,
      `read-secret`,
      `remove-secret`,
      `remove-stale-secret`,
      `write-secret`,
      `write-file`,
      `prepare-directory`,
    ]),
    aB = class extends F()(`ServerSettingsError`, {
      settingsPath: y,
      operation: iB,
      providerInstanceId: g(y),
      environmentVariable: g(y),
      cause: g(M()),
    }) {
      get message() {
        let e =
            this.providerInstanceId === void 0 ? `` : ` for provider ${this.providerInstanceId}`,
          t =
            this.environmentVariable === void 0
              ? ``
              : ` and environment variable ${this.environmentVariable}`;
        return `Server settings ${this.operation} failed${e}${t} at ${this.settingsPath}.`;
      }
    };
  ({ ...rB, ...Dz });
  var oB = S({ instanceId: h(q), model: h(L), options: h(by) }),
    sB = S({
      setModelFavorites: h(w(S({ ...wz.fields, favorite: x }))),
      setModelsHidden: h(w(S({ ...wz.fields, hidden: x }))),
      setProviderModelOrder: h(S({ provider: q, modelOrder: w(I) })),
      favorites: h(Tz),
      providerModelPreferences: h(Ez),
      migrateModelPreferences: h(S({ favorites: h(Tz), providerModelPreferences: h(Ez) })),
      worktreeCleanup: h(
        D(
          T([
            S({ mode: _(`off`) }),
            S({
              mode: _(`custom`),
              rules: S({
                worktreeKeepWhen: h(Yz),
                worktreeAfterDays: h(Jz),
                worktreeOnMerge: h(x),
                worktreeOnDelete: h(x),
                worktreeUnchanged: h(x),
              }),
            }),
          ]),
        ),
      ),
      storageCleanup: h(
        S({
          worktreeKeepWhen: h(Yz),
          worktreeAfterDays: h(Jz),
          worktreeOnMerge: h(x),
          worktreeOnDelete: h(x),
          worktreeUnchanged: h(x),
          browserArtifactsAfterDays: h(Jz),
          logsAfterDays: h(Jz),
        }),
      ),
      worktreesDirectory: h(I),
      responseStreamingMode: h(qz),
      enableProviderUpdateChecks: h(x),
      continueThreadsAfterServerUpdate: h(x),
      enableAgentBrowserAccess: h(x),
      projectAgentBrowserAccessOverrides: h(C(H, D(x))),
      defaultAutoPull: h(x),
      defaultProjectScripts: h(w(tD)),
      projectScriptOverrides: h(C(H, D(w(tD)))),
      projectAutoPullOverrides: h(C(H, D(x))),
      defaultModelSelection: h(D(bw)),
      defaultRuntimeMode: h(VE),
      projectSettingsOverrides: h(C(H, D(Qz))),
      enableAgentDeviceAccess: h(x),
      enableDeviceSupport: h(x),
      deviceOnboardingCompleted: h(x),
      deviceHosts: h(II),
      sidebarAutoSettleAfterDays: h(D(WR)),
      sidebarAutoSettleOnMerge: h(x),
      autoResumeLimitedThreads: h(x),
      snoozeLimitedThreads: h(x),
      backgroundActivity: h(
        S({ schemaVersion: h(_(1)), profile: h(Wz), baseProfile: h(Hz), overrides: h(Gz) }),
      ),
      automaticGitFetchInterval: h(p_),
      providerHealthRefreshInterval: h(p_),
      backgroundActivityProfile: h(Hz),
      environmentIcon: h(D(rS)),
      defaultThreadEnvMode: h(D(aS)),
      newWorktreesStartFromOrigin: h(x),
      worktreeSubmodules: h(D(oS)),
      addProjectBaseDirectory: h(I),
      textGenerationModelSelection: h(oB),
      branchNamingMode: h(zz),
      branchNamePrefix: h(I),
      branchNameInstructions: h(I),
      removeAgentCreditsOnMerge: h(x),
      sourceControlWritingStyle: h(
        S({ mode: h(Lz), customInstructions: h(I), followChangeRequestTemplates: h(x) }),
      ),
      sourceControlWriterModelSelection: h(D(bw)),
      reviewGuidesEnabled: h(x),
      reviewGuideModelSelection: h(D(bw)),
      reviewGuideInstructions: h(I),
      pullRequestMergeMethod: h(D(cT)),
      observability: h(S({ otlpTracesUrl: h(I), otlpMetricsUrl: h(I), otlpLogsUrl: h(I) })),
      sourceControlHosts: h(C(y, Fz)),
      providerInstances: h(C(q, iy)),
      usageLimitSources: h(C(KD, D(Pz))),
      cursorKeychainUsageEnabled: h(x),
      usagePriceOverrides: h(C(L, D(kz))),
      usageModelMappings: h(C(L, D(L))),
    });
  S({
    notificationMode: h(dz),
    inAppNotificationsEnabled: h(x),
    diffColorScheme: h(Sz),
    chatWidth: h(Cz),
    loadBalancingEnabled: h(x),
    loadBalancingWeights: h(xz),
    appearanceContrast: h(JR),
    panelAnimationDurationMs: h(XR),
    browserDefaultViewport: h(VL),
    browserDefaultZoomFactor: h(UL),
    browserDefaultAppearance: h(WL),
    browserRecordingFrameRate: h(vz),
    browserRecordingShowKeyPresses: h(x),
    browserRecordingShowMousePresses: h(x),
    browserLinkTarget: h(yz),
    browserAutoShowFloatingPreview: h(x),
    browserProfiles: h(w(AI)),
    browserDefaultProfileId: h(kI),
    confirmQuit: h(fz),
    confirmThreadArchive: h(x),
    confirmThreadDelete: h(x),
    confirmThreadUnpin: h(x),
    diffFilesCollapsed: h(x),
    diffIgnoreWhitespace: h(x),
    diffLayout: h(PR),
    environmentIdentificationMode: h(rz),
    glassOpacity: h(KR),
    onboardingCompletedAt: h(D(y)),
    fontSizeInterface: h(QR),
    fontSizePrompt: h($R),
    fontSizeCode: h(ez),
    fontSizeTerminal: h(tz),
    fontFamilyCode: h(hz),
    fontFamilyComposer: h(hz),
    fontFamilySans: h(hz),
    fontFamilyTerminal: h(hz),
    fontSmoothing: h(x),
    persistComposerContextStrip: h(x),
    favorites: h(Tz),
    providerModelPreferences: h(Ez),
    pullRequestMergeMethodOverrides: h(C(L, cT)),
    planModeEnabled: h(x),
    contextWindowMeterEnabled: h(x),
    composerCollapseOnScroll: h(x),
    composerRichTextEnabled: h(x),
    sendShortcut: h(E([`enter`, `mod-enter-multiline`, `mod-enter`])),
    followUpBehavior: h(E([`queue`, `steer`])),
    proactivePanelsEnabled: h(x),
    showSkillsInSlashMenu: h(x),
    legacySidebarEnabled: h(x),
    sidebarWorkingShelfEnabled: h(x),
    sidebarProjectGroupingMode: h(BR),
    sidebarProjectGroupingOverrides: h(C(L, BR)),
    sidebarProjectSortOrder: h(IR),
    sidebarThreadSortOrder: h(RR),
    sidebarThreadPreviewCount: h(HR),
    timestampFormat: h(MR),
    usageModelAliases: h(w(_R)),
    snapShotEnabled: h(x),
    snapShotIncludeAccessibility: h(x),
    snapShotShortcut: h(sz),
    snapShotPlaySound: h(x),
    snapShotSound: h(cz),
    snapShotFlash: h(x),
    snapShotAnimations: h(x),
    wordWrap: h(x),
  });
  var cB = S({
      id: y,
      label: y,
      destructive: h(x),
      disabled: h(x),
      header: h(x),
      icon: h(y),
      separatorBefore: h(x),
      checked: h(x),
      children: h(w(fg(() => cB))),
    }),
    lB = E([
      `disabled`,
      `idle`,
      `checking`,
      `up-to-date`,
      `available`,
      `downloading`,
      `downloaded`,
      `error`,
    ]),
    uB = E([`arm64`, `x64`, `other`]);
  E([`light`, `dark`, `system`]);
  var dB = E([`latest`, `nightly`]);
  S({ baseName: y, stageLabel: E([`Alpha`, `Dev`, `Nightly`]), displayName: y });
  var fB = E([`direct`, `portal`, `unavailable`]),
    pB = S({
      status: E([
        `not-installed`,
        `disabled`,
        `enabled`,
        `restart-required`,
        `update-required`,
        `extensions-disabled`,
        `unsupported`,
        `error`,
      ]),
      message: y,
    }),
    mB = S({
      status: E([`not-installed`, `update-required`, `ready`, `error`]),
      message: y,
      feedbackAvailable: g(x),
    });
  (E([
    `install-extension`,
    `enable-extension`,
    `disable-extension`,
    `install-kde-helper`,
    `remove-kde-helper`,
    `install-hyprland-helper`,
    `remove-hyprland-helper`,
    `test-mac-capture`,
    `allow-screen-recording`,
    `allow-accessibility`,
    `retry-shortcut`,
  ]),
    S({ operation: E([`install`, `remove`]), chooseFile: x, shortcut: g(y.check(j(80))) }),
    S({
      id: y,
      path: y,
      resolvedPath: y,
      before: y,
      after: y,
      shortcut: y,
      operation: E([`install`, `remove`]),
    }),
    S({ backupPath: D(y), warning: D(y) }),
    S({
      mode: fB,
      windows: g(x),
      linuxDesktop: g(E([`gnome`, `kde`, `niri`, `hyprland`])),
      linuxBackend: g(
        E([`screenshot-portal`, `gnome-extension`, `niri`, `kde`, `hyprland`, `picker`]),
      ),
      linuxFeedbackAvailable: g(x),
      shortcut: sz,
      shortcutRegistered: x,
      shortcutPending: g(x),
      shortcutCanRetry: g(x),
      shortcutLabel: g(y),
      shortcutMessage: D(y),
      shortcutBinding: g(y),
      shortcutConfigPath: g(y),
      shortcutActionRegistered: g(x),
      gnomeExtension: g(pB),
      kdeHelper: g(mB),
      hyprlandHelper: g(mB),
      macPermissions: g(S({ screenRecording: x, accessibility: x })),
      shortcutVerified: g(x),
      message: D(y),
    }),
    S({ available: x, message: D(y) }));
  var hB = L.check(j(64), Mg(/^[a-f0-9-]+$/i));
  (T([
    S({ type: _(`requested`), id: hB }),
    S({ type: _(`started`), id: hB }),
    S({ type: _(`ready`), id: hB }),
    S({ type: _(`failed`), id: g(hB) }),
    S({ type: _(`shortcut-changed`) }),
  ]),
    S({
      ...S({ id: hB, name: y, mimeType: _(`image/png`), sizeBytes: A, source: ew }).fields,
      dataUrl: y,
    }),
    S({
      id: hB,
      viewportFrame: S({ x: b, y: b, width: b, height: b }),
      backgroundColor: y,
      borderColor: y,
      borderWidth: b,
      cornerRadius: b,
      details: g(
        S({
          appName: ew.fields.appName,
          windowTitle: ew.fields.windowTitle,
          appIconDataUrl: ew.fields.appIconDataUrl,
        }),
      ),
    }));
  var gB = S({ version: y, items: w(y), totalItems: b }),
    _B = S({
      enabled: x,
      status: lB,
      channel: dB,
      currentVersion: y,
      hostArch: uB,
      appArch: uB,
      runningUnderArm64Translation: x,
      availableVersion: D(y),
      downloadedVersion: D(y),
      releaseNotes: w(gB),
      omittedReleaseCount: b,
      downloadPercent: D(b),
      checkedAt: D(y),
      message: D(y),
      errorContext: D(E([`check`, `download`, `install`])),
      canRetry: x,
    });
  (S({ supported: x, installedPath: D(y), onPath: x, shadowedBy: h(y) }),
    S({ accepted: x, completed: x, state: _B }),
    S({ checked: x, state: _B }),
    S({
      id: y,
      label: y,
      runningDistro: h(D(y)),
      httpBaseUrl: D(y),
      wsBaseUrl: D(y),
      bootstrapToken: h(y),
    }));
  var vB = S({ alias: y, hostname: y, username: D(y), port: D(b) }),
    yB = E([`ssh-config`, `known-hosts`]);
  S({ alias: y, hostname: y, username: D(y), port: D(b), source: yB });
  var bB = S({
      target: vB,
      httpBaseUrl: y,
      wsBaseUrl: y,
      pairingToken: D(y),
      remotePort: h(b),
      remoteServerKind: h(E([`external`, `managed`])),
    }),
    xB = S({ type: _(`ssh-password-prompt-cancelled`), message: y });
  (S({ target: vB, options: h(S({ issuePairingToken: h(x) })) }),
    T([bB, xB]),
    S({ httpBaseUrl: y }),
    S({ httpBaseUrl: y, bearerToken: y }),
    S({ httpBaseUrl: y, credential: y }),
    S({ requestId: y, password: D(y) }),
    S({
      environmentId: U,
      label: y,
      wsBaseUrl: y,
      httpBaseUrl: y,
      createdAt: y,
      lastConnectedAt: D(y),
      desktopSsh: h(vB),
      relayManaged: h(S({ relayUrl: y })),
    }),
    S({
      mode: E([`local-only`, `network-accessible`]),
      endpointUrl: D(y),
      advertisedHost: D(y),
      tailscaleServeEnabled: x,
      tailscaleServePort: b,
    }),
    S({ initialPath: h(D(y)), targetEnvironmentId: h(y) }),
    S({ name: y, size: b, text: y }));
  var SB = S({ name: y, isDefault: x, version: E([1, 2]) });
  S({ enabled: x, distro: D(y), available: x, wslOnly: x, distros: w(SB), preflightError: D(y) });
  var CB = E([`system`, `light`, `dark`]),
    wB = y.check(jg()).check(Yg());
  (T([
    S({
      type: _(`pointer`),
      phase: E([`move`, `down`, `up`, `click`]),
      x: Ng,
      y: Ng,
      width: Ng.check(Vg(0)),
      height: Ng.check(Vg(0)),
    }),
    S({ type: _(`key`), label: D(y.check(j(100))), held: x, width: Ng.check(Vg(0)) }),
    S({ type: _(`clear`) }),
  ]),
    S({ partition: y, webPreferences: y, preloadUrl: D(y) }));
  var TB = S({
    colorScheme: E([`light`, `dark`]),
    radius: y,
    background: y,
    foreground: y,
    popover: y,
    popoverForeground: y,
    primary: y,
    primaryForeground: y,
    muted: y,
    mutedForeground: y,
    accent: y,
    accentForeground: y,
    border: y,
    input: y,
    ring: y,
    fontSans: y,
    fontMono: y,
  });
  (S({ id: y, tabId: wB, path: y, mimeType: y, sizeBytes: A, createdAt: y }),
    S({ id: y, tabId: wB, path: y, mimeType: _(`image/png`), sizeBytes: A, createdAt: y }));
  var EB = S({ functionName: D(y), fileName: D(y), lineNumber: D(b), columnNumber: D(b) }),
    DB = S({
      pageUrl: y,
      pageTitle: D(y),
      tagName: y,
      selector: D(y),
      htmlPreview: y,
      componentName: D(y),
      source: D(EB),
      stack: w(EB),
      styles: y,
      pickedAt: y,
    }),
    OB = S({ x: b, y: b, width: b, height: b }),
    kB = S({ x: b, y: b }),
    AB = S({ id: y, element: DB, rect: OB }),
    jB = S({ id: y, rect: OB }),
    MB = S({ id: y, color: y, width: b, points: w(kB), bounds: OB }),
    NB = S({ targetId: y, selector: D(y), property: y, previousValue: y, value: y }),
    PB = S({ dataUrl: y, width: b, height: b, cropRect: OB });
  (S({
    annotation: S({
      id: y,
      pageUrl: y,
      pageTitle: D(y),
      comment: y,
      elements: w(AB),
      regions: w(jB),
      strokes: w(MB),
      styleChanges: w(NB),
      screenshot: D(PB),
      createdAt: y,
    }),
    submission: E([`attach`, `send`]),
    screenshotFailed: h(x),
  }),
    S({ tabId: wB }),
    S({
      tabId: wB,
      zoomFactor: g(b.check(Vg(0))),
      colorScheme: g(CB),
      serverTab: g(S({ threadId: L, tabId: L })),
    }),
    S({ tabId: wB, webContentsId: A.check(Vg(0)) }),
    S({ tabId: wB, url: y }),
    S({ environmentId: U, profileId: g(kI) }),
    S({ environmentId: U, profileId: g(kI) }),
    S({ tabId: wB, colorScheme: CB }),
    S({ tabId: wB, zoomFactor: b.check(Vg(0)) }),
    S({ tabId: wB, audioMuted: x }),
    S({ theme: TB }),
    S({ tabId: wB, enabled: x }),
    S({ path: y.check(jg()).check(Yg()) }),
    S({ tabId: wB, mimeType: y.check(jg()).check(Yg()), data: a_ }),
    E([`full-disk-access`]));
  var FB = L,
    IB = A.check(Hg(1)).check(Wg(1e3)),
    LB = A.check(Hg(1)).check(Wg(500)),
    RB = FB.check(j(128)),
    zB = C(y.check(Mg(/^[A-Za-z_][A-Za-z0-9_]*$/)).check(j(128)), y.check(j(8192))).check(Zg(128)),
    BB = S({ threadId: FB }),
    VB = S({ ...BB.fields, terminalId: RB }),
    HB = S({
      ...VB.fields,
      cwd: FB,
      worktreePath: g(D(FB)),
      cols: g(IB),
      rows: g(LB),
      env: g(zB),
      providerInstanceId: g(q),
    }),
    UB = S({
      ...VB.fields,
      cwd: g(FB),
      worktreePath: g(D(FB)),
      cols: g(IB),
      rows: g(LB),
      env: g(zB),
      providerInstanceId: g(q),
      restartIfNotRunning: g(x),
    }),
    WB = VB,
    GB = S({ ...VB.fields, data: y.check(Yg()).check(j(65536)) }),
    KB = S({ ...VB.fields, cols: IB, rows: LB }),
    qB = VB,
    JB = S({
      ...VB.fields,
      cwd: FB,
      worktreePath: g(D(FB)),
      cols: IB,
      rows: LB,
      env: g(zB),
      providerInstanceId: g(q),
    }),
    YB = S({ ...BB.fields, terminalId: g(RB), deleteHistory: g(x) }),
    XB = E([`starting`, `running`, `exited`, `error`]),
    ZB = S({
      threadId: y.check(Yg()),
      terminalId: y.check(Yg()),
      cwd: y.check(Yg()),
      worktreePath: D(FB),
      status: XB,
      pid: D(A.check(Vg(0))),
      history: y,
      exitCode: D(A),
      exitSignal: D(A),
      label: y.check(j(128)),
      updatedAt: y,
      sequence: g(A.check(Hg(0))),
    }),
    QB = S({
      threadId: y.check(Yg()),
      terminalId: y.check(Yg()),
      cwd: y.check(Yg()),
      worktreePath: D(FB),
      status: XB,
      pid: D(A.check(Vg(0))),
      exitCode: D(A),
      exitSignal: D(A),
      hasRunningSubprocess: x,
      label: y.check(j(128)),
      updatedAt: y,
    }),
    $B = T([
      S({ type: _(`snapshot`), terminals: w(QB) }),
      S({ type: _(`upsert`), terminal: QB }),
      S({ type: _(`remove`), threadId: y.check(Yg()), terminalId: y.check(Yg()) }),
    ]),
    eV = S({ threadId: y.check(Yg()), terminalId: y.check(Yg()), sequence: g(A.check(Hg(0))) }),
    tV = S({ ...eV.fields, type: _(`started`), snapshot: ZB }),
    nV = S({ ...eV.fields, type: _(`output`), data: y }),
    rV = S({ ...eV.fields, type: _(`exited`), exitCode: D(A), exitSignal: D(A) }),
    iV = S({ ...eV.fields, type: _(`closed`) }),
    aV = S({ ...eV.fields, type: _(`error`), message: y.check(Yg()) }),
    oV = S({ ...eV.fields, type: _(`cleared`) }),
    sV = S({ ...eV.fields, type: _(`restarted`), snapshot: ZB }),
    cV = S({ ...eV.fields, type: _(`activity`), hasRunningSubprocess: x, label: y.check(j(128)) }),
    lV = T([tV, nV, rV, iV, aV, oV, sV, cV]),
    uV = T([S({ type: _(`snapshot`), snapshot: ZB }), nV, rV, iV, aV, oV, sV, cV]),
    dV = T([
      T([
        class extends F()(`TerminalCwdNotFoundError`, { cwd: y }) {
          get message() {
            return `Terminal cwd does not exist: ${this.cwd}`;
          }
        },
        class extends F()(`TerminalCwdNotDirectoryError`, { cwd: y }) {
          get message() {
            return `Terminal cwd is not a directory: ${this.cwd}`;
          }
        },
        class extends F()(`TerminalCwdStatError`, { cwd: y, cause: M() }) {
          get message() {
            return `Failed to access terminal cwd: ${this.cwd}`;
          }
        },
      ]),
      class extends F()(`TerminalHistoryError`, {
        operation: E([`read`, `truncate`, `migrate`]),
        threadId: y,
        terminalId: y,
        cause: g(M()),
      }) {
        get message() {
          return `Failed to ${this.operation} terminal history for thread: ${this.threadId}, terminal: ${this.terminalId}`;
        }
      },
      class extends F()(`TerminalSessionLookupError`, { threadId: y, terminalId: y }) {
        get message() {
          return `Unknown terminal thread: ${this.threadId}, terminal: ${this.terminalId}`;
        }
      },
      class extends F()(`TerminalProviderInstanceNotFoundError`, { providerInstanceId: q }) {
        get message() {
          return `Provider instance is not available: ${this.providerInstanceId}`;
        }
      },
      class extends F()(`TerminalProviderEnvironmentError`, { providerInstanceId: q, cause: M() }) {
        get message() {
          return `Could not prepare the terminal environment for provider instance: ${this.providerInstanceId}`;
        }
      },
      class extends F()(`TerminalNotRunningError`, { threadId: y, terminalId: y }) {
        get message() {
          return `Terminal is not running for thread: ${this.threadId}, terminal: ${this.terminalId}`;
        }
      },
      class extends F()(`TerminalWriteError`, {
        threadId: y,
        terminalId: y,
        terminalPid: b,
        cause: M(),
      }) {
        get message() {
          return `Failed to write to terminal for thread: ${this.threadId}, terminal: ${this.terminalId}, PID: ${this.terminalPid}`;
        }
      },
      class extends F()(`TerminalResizeError`, {
        threadId: y,
        terminalId: y,
        terminalPid: b,
        cols: IB,
        rows: LB,
        cause: M(),
      }) {
        get message() {
          return `Failed to resize terminal for thread: ${this.threadId}, terminal: ${this.terminalId}, PID: ${this.terminalPid} to ${this.cols}x${this.rows}`;
        }
      },
    ]),
    fV = E([`connecting`, `ready`, `running`, `error`, `closed`]);
  (S({
    provider: ny,
    providerInstanceId: g(q),
    status: fV,
    runtimeMode: VE,
    cwd: g(L),
    model: g(L),
    threadId: V,
    resumeCursor: g(v),
    activeTurnId: g(W_),
    createdAt: B,
    updatedAt: B,
    lastError: g(L),
  }),
    S({
      threadId: V,
      provider: g(ny),
      providerInstanceId: g(q),
      cwd: g(L),
      title: g(L),
      modelSelection: g(bw),
      resumeCursor: g(v),
      approvalPolicy: g(zE),
      sandboxMode: g(BE),
      runtimeMode: VE,
    }),
    S({
      threadId: V,
      continuation: g(x),
      input: g(L.check(j(zC))),
      attachments: g(w(ow).check(k((e) => sw(e) ?? !0))),
      modelSelection: g(bw),
      interactionMode: g(UE),
    }),
    S({ threadId: V, turnId: W_, resumeCursor: g(v) }),
    S({ threadId: V, turnId: g(W_) }),
    S({ threadId: V }),
    S({ threadId: V, requestId: ov, decision: GE }),
    S({ threadId: V, requestId: ov, answers: qE, attachmentsByQuestionId: g(JE) }));
  var pV = S({ threadId: V, reason: g(L) }),
    mV = S({ feedbackId: L }),
    hV = class extends F()(`ProviderUploadFeedbackError`, { threadId: V, cause: g(M()) }) {
      get message() {
        return `Failed to upload feedback for thread ${this.threadId}.`;
      }
    };
  S({
    id: H_,
    kind: E([`session`, `notification`, `request`, `error`]),
    provider: ny,
    providerInstanceId: g(q),
    threadId: V,
    createdAt: B,
    method: L,
    message: g(L),
    turnId: g(W_),
    itemId: g(X_),
    requestId: g(ov),
    requestKind: g(WE),
    textDelta: g(y),
    payload: g(v),
  });
  var gV = S({ instanceId: q }),
    _V = S({
      authorizationUrl: y.check(j(16384)),
      returnUrl: y.check(j(4096)),
      environmentId: U,
      instanceId: q,
      flowId: L.check(j(128)),
    }),
    vV = T([S({ phase: _(`ready`) }), S({ phase: _(`finished`), callbackUrl: y.check(j(16384)) })]),
    yV = L.check(j(128)),
    bV = S({
      id: yV,
      accountEmail: g(L),
      name: L,
      description: D(y),
      type: E([`agent`, `terminal`, `credentials`]),
    }),
    xV = T([
      S({
        type: _(`browser`),
        id: yV,
        url: L.check(j(16384)),
        requiresConsent: x,
        acceptsCallback: h(x),
      }),
      S({ type: _(`deviceCode`), id: yV, url: L.check(j(16384)), userCode: L.check(j(256)) }),
      S({
        type: _(`terminal`),
        id: yV,
        output: y.check(j(16384)),
        outputOffset: h(A.check(Hg(0))),
      }),
      S({
        type: _(`credentials`),
        id: yV,
        fields: w(S({ name: yV, label: L, secret: x })).check(j(16)),
      }),
    ]),
    SV = T([
      S({ type: _(`browser`), action: E([`accept`, `decline`]) }),
      S({
        type: _(`terminal`),
        data: y.check(j(4096)),
        size: h(
          S({
            cols: A.check(Gg({ minimum: 1, maximum: 500 })),
            rows: A.check(Gg({ minimum: 1, maximum: 200 })),
          }),
        ),
      }),
      S({ type: _(`credentials`), values: C(yV, y.check(j(16384))).check(Zg(16)) }),
    ]),
    CV = S({
      instanceId: q,
      methodId: h(yV),
      returnUrl: h(y),
      callbackMode: h(E([`server`, `client`])),
    }),
    wV = S({ instanceId: q, flowId: yV, interactionId: yV, response: SV }),
    TV = S({
      instanceId: q,
      phase: E([`idle`, `starting`, `waiting`, `verifying`, `succeeded`, `failed`, `cancelled`]),
      flowId: D(yV),
      authorizationUrl: D(y),
      expiresAt: D(B),
      message: D(y),
      methods: h(P_(bV).check(j(32))),
      interaction: j_(D(xV)),
      credentialOwner: j_(E([`provider`, `t3`])),
    }),
    EV = S({ instanceId: q, flowId: yV, callbackUrl: L.check(j(16384)) }),
    DV = S({ instanceId: q, flowId: yV }),
    OV = A.check(Hg(0)),
    kV = S({
      driver: ny,
      operationId: D(yV),
      phase: E([
        `idle`,
        `downloading`,
        `extracting`,
        `verifying`,
        `succeeded`,
        `failed`,
        `cancelled`,
      ]),
      downloadedBytes: OV,
      totalBytes: D(OV),
      version: D(L),
      installedVersion: D(L),
      executablePath: h(D(L)),
      source: h(D(E([`managed`, `local`]))),
      canRemove: x,
      message: D(y),
    }),
    AV = S({ instanceId: q, operationId: yV }),
    jV = class extends F()(`ProviderSetupError`, {
      instanceId: q,
      operation: y,
      detail: y,
      cause: g(M()),
    }) {
      get message() {
        return this.detail;
      }
    },
    MV = S({
      clientId: y.check(Mg(/^oaiapp_[\w-]+$/u)),
      subject: h(y),
      email: h(D(y)),
      redirectUri: h(
        y.check(Mg(/^http:\/\/(?:127\.0\.0\.1|localhost):[1-9]\d{0,4}\/auth\/callback$/u)),
      ),
      connectionLabel: h(y),
      sharingEnabled: h(x),
      idTokenHint: h(y.check(j(16384))),
    }),
    NV = S({
      registration: MV,
      credentials: S({
        clientId: y,
        accessToken: Qg.check(j(16384)),
        refreshToken: D(y.check(j(16384))),
        idToken: Qg.check(j(16384)),
        issuer: y,
        expiresAt: Ng,
        earliestRefreshAt: D(Ng),
        scopes: w(y),
        subject: y,
        email: D(y),
      }),
    }),
    PV = S({ instanceId: q, methodId: y }),
    FV = S({ instanceId: q, profile: NV }),
    IV = S({
      instanceId: q,
      environmentId: U,
      attemptId: y.check(j(128)),
      returnUrl: y.check(j(4096)),
      profile: D(MV),
    }),
    LV = T([S({ phase: _(`auth`), state: TV }), S({ phase: _(`finished`), profile: NV })]);
  E([`direct-path`, `goto`, `line-column`]);
  var RV = [
      {
        id: `cursor`,
        label: `Cursor`,
        commands: [`cursor`],
        baseArgs: [`--classic`],
        launchStyle: `goto`,
        remoteScheme: `cursor`,
      },
      { id: `trae`, label: `Trae`, commands: [`trae`], launchStyle: `goto` },
      { id: `kiro`, label: `Kiro`, commands: [`kiro`], baseArgs: [`ide`], launchStyle: `goto` },
      {
        id: `vscode`,
        label: `VS Code`,
        commands: [`code`],
        launchStyle: `goto`,
        remoteScheme: `vscode`,
      },
      {
        id: `vscode-insiders`,
        label: `VS Code Insiders`,
        commands: [`code-insiders`],
        launchStyle: `goto`,
        remoteScheme: `vscode-insiders`,
      },
      {
        id: `vscodium`,
        label: `VSCodium`,
        commands: [`codium`],
        launchStyle: `goto`,
        remoteScheme: `vscodium`,
      },
      {
        id: `zed`,
        label: `Zed`,
        commands: [`zed`, `zeditor`],
        launchStyle: `direct-path`,
        remoteScheme: `zed`,
      },
      {
        id: `antigravity`,
        label: `Antigravity`,
        commands: [`antigravity-ide`, `agy-ide`],
        launchStyle: `goto`,
      },
      {
        id: `idea`,
        label: `IntelliJ IDEA`,
        commands: [`idea`],
        launchStyle: `line-column`,
        jetbrainsProductCode: `IU`,
      },
      {
        id: `aqua`,
        label: `Aqua`,
        commands: [`aqua`],
        launchStyle: `line-column`,
        jetbrainsProductCode: `QA`,
      },
      {
        id: `clion`,
        label: `CLion`,
        commands: [`clion`],
        launchStyle: `line-column`,
        jetbrainsProductCode: `CL`,
      },
      {
        id: `datagrip`,
        label: `DataGrip`,
        commands: [`datagrip`],
        launchStyle: `line-column`,
        jetbrainsProductCode: `DB`,
      },
      {
        id: `dataspell`,
        label: `DataSpell`,
        commands: [`dataspell`],
        launchStyle: `line-column`,
        jetbrainsProductCode: `DS`,
      },
      {
        id: `goland`,
        label: `GoLand`,
        commands: [`goland`],
        launchStyle: `line-column`,
        jetbrainsProductCode: `GO`,
      },
      {
        id: `phpstorm`,
        label: `PhpStorm`,
        commands: [`phpstorm`],
        launchStyle: `line-column`,
        jetbrainsProductCode: `PS`,
      },
      {
        id: `pycharm`,
        label: `PyCharm`,
        commands: [`pycharm`],
        launchStyle: `line-column`,
        jetbrainsProductCode: `PY`,
      },
      {
        id: `rider`,
        label: `Rider`,
        commands: [`rider`],
        launchStyle: `line-column`,
        jetbrainsProductCode: `RD`,
      },
      {
        id: `rubymine`,
        label: `RubyMine`,
        commands: [`rubymine`],
        launchStyle: `line-column`,
        jetbrainsProductCode: `RM`,
      },
      {
        id: `rustrover`,
        label: `RustRover`,
        commands: [`rustrover`],
        launchStyle: `line-column`,
        jetbrainsProductCode: `RR`,
      },
      {
        id: `webstorm`,
        label: `WebStorm`,
        commands: [`webstorm`],
        launchStyle: `line-column`,
        jetbrainsProductCode: `WS`,
      },
      { id: `file-manager`, label: `File Manager`, commands: null, launchStyle: `direct-path` },
    ],
    zV = E(RV.map((e) => e.id)),
    BV = E([`finder`, `file-explorer`, `files`]),
    VV = S({ cwd: L, editor: zV, reveal: g(x) }),
    HV = (e) => e.remoteScheme ?? (e.jetbrainsProductCode === void 0 ? void 0 : `jetbrains`);
  RV.flatMap((e) => (HV(e) === void 0 ? [] : [e.id]));
  var UV = S({ kind: E([`tailscale`, `mdns`]), host: L }),
    WV = class extends F()(`ExternalLauncherUnknownEditorError`, { editor: y }) {
      get message() {
        return `Unknown editor: ${this.editor}`;
      }
    },
    GV = class extends F()(`ExternalLauncherUnsupportedEditorError`, { editor: zV }) {
      get message() {
        return `Unsupported editor: ${this.editor}`;
      }
    },
    KV = class extends F()(`ExternalLauncherCommandNotFoundError`, { editor: zV, command: y }) {
      get message() {
        return `Editor command not found: ${this.command}`;
      }
    },
    qV = class extends F()(`ExternalLauncherUnsupportedTargetError`, { editor: zV }) {
      get message() {
        return `${this.editor} cannot open a path containing line breaks or double quotes`;
      }
    },
    JV = { command: y, args: w(y), cause: M() },
    YV = T([
      WV,
      GV,
      KV,
      qV,
      class extends F()(`ExternalLauncherBrowserSpawnError`, { ...JV, target: y }) {
        get message() {
          return `Failed to launch browser target '${this.target}' with '${[this.command, ...this.args].join(` `)}'`;
        }
      },
      class extends F()(`ExternalLauncherEditorSpawnError`, { ...JV, editor: zV, target: y }) {
        get message() {
          return `Failed to launch '${this.target}' in ${this.editor} with '${[this.command, ...this.args].join(` `)}'`;
        }
      },
    ]),
    XV = P_(
      T([
        S({ kind: _(`keybindings.malformed-config`), message: L }),
        S({ kind: _(`keybindings.invalid-entry`), message: L, index: b }),
      ]),
    ),
    ZV = E([`ready`, `warning`, `error`, `disabled`]),
    QV = S({
      status: E([`authenticated`, `unauthenticated`, `unknown`]),
      type: g(L),
      label: g(L),
      email: g(L),
      action: g(zy),
      canLogout: g(x),
      subscriptionSharing: g(x),
      profileId: g(L),
      workspaceId: g(L),
    }),
    $V = S({
      slug: L,
      name: L,
      shortName: g(L),
      subProvider: g(L),
      aliases: g(w(L)),
      badge: g(_(`new`)),
      isCustom: x,
      isDefault: g(x),
      isLegacy: g(x),
      capabilities: D(Cy),
    }),
    eH = S({ slug: L, name: L, badge: g(_(`new`)), minVersion: L }),
    tH = S({ hint: L }),
    nH = S({ name: L, description: g(L), input: g(tH) }),
    rH = S({
      name: L,
      description: g(L),
      path: L,
      scope: g(L),
      enabled: x,
      displayName: g(L),
      shortDescription: g(L),
      userInvocationOnly: g(x),
      userInvocable: g(x),
    }),
    iH = S({
      cwd: L,
      checkedAt: B,
      slashCommands: w(nH),
      slashCommandsPending: g(x),
      skills: w(rH),
    }),
    aH = E([`available`, `unavailable`]),
    oH = S({ groupKey: L }),
    sH = E([`unknown`, `supported`, `graceful`, `unsupported`, `broken`]),
    cH = S({
      status: sH,
      latestVersionStatus: h(sH),
      message: D(L),
      recommendedVersion: D(L),
      recommendedRange: D(L),
    }),
    lH = S({
      status: E([`unknown`, `current`, `behind_latest`]),
      currentVersion: D(L),
      latestVersion: D(L),
      updateCommand: D(L),
      canUpdate: x.pipe(O(m(!1))),
      canInstallVersion: h(x),
      checkedAt: D(B),
      message: D(L),
    }),
    uH = S({
      status: E([`idle`, `queued`, `running`, `succeeded`, `failed`, `unchanged`]),
      startedAt: D(B),
      finishedAt: D(B),
      message: D(L),
      output: D(y.check(j(1e4))),
    }),
    dH = P_(
      S({
        instanceId: q,
        driver: ny,
        displayName: g(L),
        accentColor: g(L),
        iconUrl: g(L.check(j(2048))),
        badgeLabel: g(L),
        continuation: g(oH),
        showInteractionModeToggle: g(x),
        reportsContextWindow: g(x),
        supportedRuntimeModes: g(P_(VE)),
        requiresNewThreadForModelChange: g(x),
        supportsConversationRollback: g(x),
        supportsTextGeneration: g(x),
        setup: g(S({ canAuthenticate: x, canInstall: x, documentationUrl: h(L.check(j(2048))) })),
        nativeSessions: g(S({ canList: x, canLoad: x, canResume: x, canDelete: g(x) })),
        configurableProviders: g(x),
        runtimePaths: h(S({ homePath: L, shadowHomePath: D(L) })),
        enabled: x,
        installed: x,
        version: D(L),
        status: ZV,
        auth: QV,
        checkedAt: B,
        message: g(L),
        availability: g(aH),
        unavailableReason: g(L),
        models: w($V),
        updateRequiredModels: h(w(eH)),
        slashCommands: w(nH).pipe(O(m([]))),
        skills: w(rH).pipe(O(m([]))),
        workspaceSnapshots: h(w(iH)),
        usageLimits: g(YD),
        versionAdvisory: h(lH),
        compatibilityAdvisory: h(cH),
        updateState: h(uH),
      }),
    ),
    fH = S({
      logsDirectoryPath: L,
      localTracingEnabled: x,
      otlpTracesUrl: g(L),
      otlpTracesEnabled: x,
      otlpMetricsUrl: g(L),
      otlpMetricsEnabled: x,
      otlpLogsUrl: g(L),
      otlpLogsEnabled: x.pipe(O(m(!1))),
    }),
    pH = S({ signal: E([`traces`, `metrics`, `logs`]), url: L.check(Mg(/^https?:\/\/./i)) }),
    mH = T([
      Dg(`Accepted`, { latencyMs: b }),
      Dg(`Rejected`, { status: b }),
      Dg(`Unreachable`, { timedOut: x }),
    ]),
    hH = E([`trace-file-not-found`, `trace-file-read-failed`]),
    gH = S({
      name: L,
      count: R,
      failureCount: R,
      totalDurationMs: b,
      averageDurationMs: b,
      maxDurationMs: b,
    }),
    _H = S({ name: L, cause: L, count: R, lastSeenAt: N, traceId: L, spanId: L }),
    vH = S({ name: L, cause: L, durationMs: b, endedAt: N, traceId: L, spanId: L }),
    yH = S({ name: L, durationMs: b, endedAt: N, traceId: L, spanId: L }),
    bH = S({ spanName: L, level: L, message: L, seenAt: N, traceId: L, spanId: L }),
    xH = S({
      traceFilePath: L,
      scannedFilePaths: w(L),
      readAt: N,
      recordCount: R,
      parseErrorCount: R,
      firstSpanAt: m_(N),
      lastSpanAt: m_(N),
      failureCount: R,
      interruptionCount: R,
      slowSpanThresholdMs: R,
      slowSpanCount: R,
      logLevelCounts: C(L, R),
      topSpansByCount: w(gH),
      slowestSpans: w(yH),
      commonFailures: w(_H),
      latestFailures: w(vH),
      latestWarningAndErrorLogs: w(bH),
      partialFailure: m_(x),
      error: m_(S({ kind: hH, message: L })),
    }),
    SH = E([`SIGINT`, `SIGKILL`]),
    CH = S({
      serverPid: z,
      readAt: N,
      processCount: R,
      totalRssBytes: R,
      totalCpuPercent: b,
      processes: w(
        S({
          pid: z,
          startTimeMs: R,
          ppid: R,
          pgid: m_(A),
          status: L,
          cpuPercent: b,
          rssBytes: R,
          elapsed: L,
          command: L,
          depth: R,
          childPids: w(z),
        }),
      ),
      error: m_(S({ message: L })),
    }),
    wH = S({ windowMs: R, bucketMs: R }),
    TH = S({
      startedAt: N,
      endedAt: N,
      avgCpuPercent: b,
      maxCpuPercent: b,
      maxRssBytes: R,
      maxProcessCount: R,
    }),
    EH = S({
      processKey: L,
      pid: z,
      ppid: R,
      command: L,
      depth: R,
      isServerRoot: x,
      firstSeenAt: N,
      lastSeenAt: N,
      currentCpuPercent: b,
      avgCpuPercent: b,
      maxCpuPercent: b,
      cpuSecondsApprox: b,
      currentRssBytes: R,
      maxRssBytes: R,
      sampleCount: R,
    }),
    DH = E([
      `ProcessDiagnosticsQueryTimeoutError`,
      `ProcessDiagnosticsQueryFailedError`,
      `ProcessDiagnosticsServerProcessSignalError`,
      `ProcessDiagnosticsNotDescendantError`,
      `ProcessDiagnosticsSignalFailedError`,
    ]),
    OH = S({
      readAt: N,
      windowMs: R,
      bucketMs: R,
      sampleIntervalMs: R,
      retainedSampleCount: R,
      totalCpuSecondsApprox: b,
      buckets: w(TH),
      topProcesses: w(EH),
      error: m_(S({ failureTag: DH, message: L })),
    }),
    kH = S({ pid: z, startTimeMs: R, signal: SH }),
    AH = S({ pid: z, signal: SH, signaled: x, message: m_(L) }),
    jH = y.check(Mg(/^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/)),
    MH = y.check(Mg(/^(?!(?:system|light|dark)$)[a-z0-9](?:[a-z0-9-]{0,47})$/)),
    NH = C(y.check(Mg(/^[a-zA-Z][a-zA-Z0-9]{0,63}$/)), L.check(j(64))),
    PH = {
      version: g(_(1)),
      name: L.check(j(48)),
      appearance: E([`light`, `dark`]),
      canvas: g(jH),
      accent: g(jH),
      colors: g(NH),
      variants: g(S({ light: g(NH), dark: g(NH) })),
    };
  S(PH);
  var FH = S({ id: MH, ...PH }),
    IH = S({ kind: E([`lan`, `tailnet`]), httpBaseUrl: L }),
    LH = S({
      environment: dS,
      auth: yx,
      cwd: L,
      keybindingsConfigPath: L,
      keybindings: TL,
      issues: XV,
      providers: dH,
      availableEditors: P_(zV),
      remoteOpenTargets: h(P_(UV)),
      directEndpoints: h(P_(IH)),
      observability: fH,
      settings: nB,
      shellResumeCompletionMarker: h(x),
      shellRevealInFileManager: h(x),
      shellRevealInFileManagerKind: h(BV),
      threadResumeCompletionMarker: h(x),
      threadSnapshotPagination: h(x),
      reasoningMessages: h(x),
      threadFind: h(x),
      threadFindProgressive: h(x),
      scratchWorkspaceRoot: h(L),
      newProjectsRoot: h(L),
      environmentThemes: g(w(FH)),
      usageLimitSources: g(QD),
    }),
    RH = S({ key: bL, command: yL, when: g(xL) }),
    zH = S({ key: bL, command: yL, when: g(xL), replace: g(RH) }),
    BH = RH,
    VH = S({ keybindings: TL, issues: XV }),
    HH = VH;
  S({ issues: XV, providers: dH, settings: g(nB) });
  var UH = S({ keybindings: TL, issues: XV }),
    WH = S({ providers: dH }),
    GH = S({ settings: nB }),
    KH = S({ version: _(1), type: _(`snapshot`), config: LH }),
    qH = S({ version: _(1), type: _(`keybindingsUpdated`), payload: UH }),
    JH = S({ version: _(1), type: _(`providerStatuses`), payload: WH }),
    YH = S({ version: _(1), type: _(`settingsUpdated`), payload: GH }),
    XH = S({ themes: w(FH) }),
    ZH = S({ version: _(1), type: _(`environmentThemesUpdated`), payload: XH }),
    QH = S({ sources: QD }),
    $H = T([
      KH,
      qH,
      JH,
      YH,
      ZH,
      S({ version: _(1), type: _(`usageLimitSourcesUpdated`), payload: QH }),
    ]),
    eU = S({
      at: B,
      environment: dS,
      updateOutcome: h(
        S({
          id: L,
          fromVersion: L,
          targetVersion: L,
          status: E([`committed`, `rolled-back`, `failed`]),
          reason: h(L),
        }),
      ),
    }),
    tU = S({
      environment: dS,
      cwd: L,
      projectName: L,
      bootstrapStatus: g(E([`pending`, `complete`])),
      bootstrapProjectId: g(H),
      bootstrapThreadId: g(V),
      bootstrapProjectCreated: g(x),
      bootstrapThreadCreated: g(x),
    }),
    nU = S({ status: T([_(`running`), _(`complete`)]), totalThreadCount: R }),
    rU = T([
      S({ version: _(1), sequence: R, type: _(`welcome`), payload: tU }),
      S({ version: _(1), sequence: R, type: _(`ready`), payload: eU }),
      S({ version: _(1), sequence: R, type: _(`legacyThreadMigration`), payload: nU }),
    ]),
    iU = S({ providers: dH }),
    aU = S({ provider: ny, targetVersion: h(L), instanceId: h(q) }),
    oU = class extends F()(`ServerProviderUpdateError`, {
      provider: ny,
      reason: L,
      cause: g(M()),
    }) {
      get message() {
        return `Provider update failed for ${this.provider}: ${this.reason}`;
      }
    },
    sU = S({ targetVersion: L, continueRunningThreads: h(x) }),
    cU = S({ targetVersion: L, method: sS, updateId: h(L), desktopUpdateToken: h(L) }),
    lU = S({ requestId: L }),
    uU = E([`downloading`, `installing`]),
    dU = T([S({ type: _(`progress`), stage: uU }), S({ type: _(`complete`), result: cU })]),
    fU = class extends F()(`ServerSelfUpdateError`, { reason: L, cause: g(M()) }) {
      get message() {
        return `Server update failed: ${this.reason}`;
      }
    },
    Z = L,
    pU = 200,
    mU = E([`commit`, `push`, `create_pr`, `commit_push`, `commit_push_pr`]),
    hU = E([`branch`, `commit`, `push`, `pr`]);
  E([
    `action_started`,
    `phase_started`,
    `hook_started`,
    `hook_output`,
    `hook_finished`,
    `action_finished`,
    `action_failed`,
  ]);
  var gU = E([`stdout`, `stderr`]),
    _U = E([`created`, `skipped_no_changes`, `skipped_not_requested`]),
    vU = E([`pushed`, `skipped_not_requested`, `skipped_up_to_date`]),
    yU = E([`created`, `skipped_not_requested`]),
    bU = E([`created`, `opened_existing`, `skipped_not_requested`]),
    xU = E([`open`, `closed`, `merged`]),
    SU = Z,
    CU = E([`open`, `closed`, `merged`]),
    wU = E([`local`, `worktree`]),
    TU = S({ kind: mU }),
    EU = T([
      S({ kind: _(`none`) }),
      S({ kind: _(`open_pr`), label: Z, url: y }),
      S({ kind: _(`run_action`), label: Z, action: TU }),
    ]),
    DU = S({ title: Z, description: g(Z), cta: EU }),
    OU = S({
      name: Z,
      isRemote: g(x),
      remoteName: g(Z),
      current: x,
      isDefault: x,
      worktreePath: Z.pipe(D),
    }),
    kU = S({ path: Z, refName: Z }),
    AU = S({ number: z, title: Z, url: y, baseBranch: Z, headBranch: Z, state: CU }),
    jU = S({ cwd: Z }),
    MU = S({ ...jU.fields, includeRemote: g(x) }),
    NU = S({ cwd: Z }),
    PU = S({
      actionId: Z,
      cwd: Z,
      action: mU,
      commitMessage: g(Z.check(j(1e4))),
      featureBranch: g(x),
      filePaths: g(w(Z).check(qg(1))),
      threadId: g(V),
      projectId: g(H),
    }),
    FU = S({
      cwd: Z,
      query: g(Z.check(j(256))),
      cursor: g(R),
      includeMatchingRemoteRefs: g(x),
      refKind: g(E([`all`, `local`, `remote`])),
      refresh: g(x),
      limit: g(z.check(Wg(pU))),
    }),
    IU = S({ cwd: Z, refName: Z, newRefName: g(Z), baseRefName: g(Z), path: D(Z) }),
    LU = S({ cwd: Z, reference: SU }),
    RU = S({ cwd: Z, reference: SU, mode: wU, threadId: g(V) }),
    zU = S({ cwd: Z, path: Z, force: g(x) }),
    BU = S({ cwd: Z, refName: Z, switchRef: g(x) }),
    VU = S({ refName: Z }),
    HU = S({ cwd: Z, refName: Z }),
    UU = S({ cwd: Z, kind: g(xw) }),
    WU = S({
      number: z,
      title: Z,
      url: y,
      baseRef: Z,
      headRef: Z,
      state: xU,
      isDraft: g(x),
      updatedAt: g(D(y)),
    }),
    GU = {
      isRepo: x,
      sourceControlProvider: g(Pw),
      hasPrimaryRemote: x,
      isDefaultRef: x,
      refName: D(Z),
      hasWorkingTreeChanges: x,
      workingTree: S({
        files: w(S({ path: Z, insertions: R, deletions: R })),
        insertions: R,
        deletions: R,
      }),
      branchChanges: g(S({ baseRef: D(Z), insertions: R, deletions: R })),
    },
    KU = { hasUpstream: x, aheadCount: R, behindCount: R, aheadOfDefaultCount: g(R), pr: D(WU) },
    qU = S(GU),
    JU = S(KU),
    YU = S({ ...GU, ...KU }),
    XU = T([
      Dg(`snapshot`, { local: qU, remote: D(JU) }),
      Dg(`localUpdated`, { local: qU }),
      Dg(`remoteUpdated`, { remote: D(JU) }),
    ]),
    ZU = S({ refs: w(OU), isRepo: x, hasPrimaryRemote: x, nextCursor: R.pipe(D), totalCount: R }),
    QU = S({ worktree: kU }),
    $U = S({ pullRequest: AU }),
    eW = S({
      pullRequest: AU,
      branch: Z,
      worktreePath: Z.pipe(D),
      isOnPullRequestHead: x.pipe(Tg(m(!0))),
    }),
    tW = S({ refName: D(Z) }),
    nW = S({
      action: mU,
      branch: S({ status: yU, name: g(Z) }),
      commit: S({ status: _U, commitSha: g(Z), subject: g(Z) }),
      push: S({ status: vU, branch: g(Z), upstreamBranch: g(Z), setUpstream: g(x) }),
      pr: S({
        status: bU,
        url: g(y),
        number: g(z),
        baseBranch: g(Z),
        headBranch: g(Z),
        title: g(Z),
      }),
      toast: DU,
    }),
    rW = S({ status: E([`pulled`, `skipped_up_to_date`]), refName: Z, upstreamRef: Z.pipe(D) }),
    iW = E([
      `authentication_failed`,
      `branch_already_exists`,
      `branch_checked_out_in_worktree`,
      `host_key_unverified`,
      `not_a_repository`,
      `path_already_exists`,
      `remote_unreachable`,
      `tag_would_be_clobbered`,
    ]),
    aW = class extends F()(`GitCommandError`, {
      operation: y,
      command: y,
      cwd: y,
      argumentCount: g(b),
      exitCode: g(b),
      stdoutLength: g(b),
      stderrLength: g(b),
      outputLength: g(b),
      reason: g(iW),
      detail: y,
      cause: g(M()),
    }) {
      get message() {
        let e = this.reason === void 0 ? `` : ` (${this.reason})`;
        return `Git command failed in ${this.operation} (${this.cwd}): ${this.detail}${e}`;
      }
    },
    oW = class extends F()(`TextGenerationError`, { operation: y, detail: y, cause: g(M()) }) {
      get message() {
        return `Text generation failed in ${this.operation}: ${this.detail}`;
      }
    },
    sW = T([
      class extends F()(`GitManagerError`, { operation: y, cwd: y, detail: y, cause: g(M()) }) {
        get message() {
          return `Git manager failed in ${this.operation}: ${this.detail}`;
        }
      },
      class extends F()(`GitPullRequestMaterializationError`, {
        cwd: Z,
        pullRequestNumber: z,
        headRepository: D(Z),
        headBranch: Z,
        localBranch: Z,
        cause: M(),
      }) {
        get message() {
          return `Failed to materialize pull request #${this.pullRequestNumber} branch ${this.headBranch} as ${this.localBranch}.`;
        }
      },
      aW,
      Zw,
      oW,
    ]),
    cW = S({ actionId: Z, cwd: Z, action: mU }),
    lW = T([
      S({ ...cW.fields, kind: _(`action_started`), phases: w(hU) }),
      S({ ...cW.fields, kind: _(`phase_started`), phase: hU, label: Z }),
      S({ ...cW.fields, kind: _(`hook_started`), hookName: Z }),
      S({ ...cW.fields, kind: _(`hook_output`), hookName: D(Z), stream: gU, text: Z }),
      S({ ...cW.fields, kind: _(`hook_finished`), hookName: Z, exitCode: D(A), durationMs: D(R) }),
      S({ ...cW.fields, kind: _(`action_finished`), result: nW }),
      S({ ...cW.fields, kind: _(`action_failed`), phase: D(hU), message: Z }),
    ]),
    uW = 1e3,
    dW = E([`connecting`, `counting`, `receiving`, `resolving`, `checkout`]),
    fW = E([`running`, `done`, `failed`, `cancelled`]),
    pW = S({
      projectId: H,
      remoteUrl: L,
      destinationPath: L,
      repository: D(Rw),
      phase: fW,
      stage: dW,
      percent: D(A.check(Gg({ minimum: 0, maximum: 100 }))),
      detail: D(y.check(j(200))),
      error: D(y.check(j(uW))),
      startedAt: B,
      endedAt: D(B),
      sequence: R,
    }),
    mW = S({}),
    hW = w(pW),
    gW = S({
      projectId: H,
      title: L,
      createdAt: B,
      provider: g(Nw),
      repository: g(L),
      remoteUrl: g(L),
      destinationPath: L,
      protocol: g(Lw),
    }),
    _W = S({ projectId: H, cwd: L, remoteUrl: L, repository: D(Rw) }),
    vW = S({ projectId: H }),
    yW = S({ applied: x }),
    bW = S({ kind: E([`tradeoff`, `contract`, `product`, `risk`, `accessibility`]), note: L }),
    xW = S({ path: L, side: E([`new`, `old`]), start: z, end: z, caption: y }),
    SW = S({ path: L, line: z, title: L, body: y }),
    CW = S({
      headSha: L,
      generatedAt: B,
      modelSelection: bw,
      stops: w(
        S({
          title: L,
          summary: y,
          attention: D(bW),
          excerpts: w(xW),
          files: w(S({ path: L, note: h(y) })),
          notes: w(SW),
          questions: w(L),
        }),
      ),
    }),
    wW = T([
      Dg(`Running`, { headSha: L, startedAt: B }),
      Dg(`Failed`, { headSha: L, failedAt: B, detail: L }),
    ]),
    TW = S({ guide: D(CW), run: D(wW), currentHeadSha: D(L) }),
    EW = class extends F()(`OrchestrationDispatchCommandError`, {
      message: L,
      cause: g(M()),
      bootstrapThreadDisposition: g(E([`deleted`, `not-created`])),
    }) {},
    DW = S({ surface: g(J_), appVersion: g(L) }),
    OW = S({
      deferredTurn: g(x),
      providerTurnId: g(L),
      providerItemId: g(X_),
      adapterKey: g(L),
      requestId: g(ov),
      ingestedAt: g(B),
      origin: g(DW),
    }),
    kW = S({
      projectId: H,
      title: L,
      workspaceRoot: L,
      repositoryIdentity: g(D(mS)),
      defaultModelSelection: D(bw),
      defaultThreadEnvMode: g(D(aS)),
      faviconPath: g(D(L)),
      projectIcon: g(D(pD)),
      scripts: w(tD),
      createdAt: B,
      updatedAt: B,
    }),
    AW = S({
      projectId: H,
      title: g(L),
      workspaceRoot: g(L),
      repositoryIdentity: g(D(mS)),
      defaultModelSelection: g(D(bw)),
      defaultThreadEnvMode: g(D(aS)),
      autoPull: g(x),
      faviconPath: g(D(L)),
      projectIcon: g(D(pD)),
      scripts: g(w(tD)),
      updatedAt: B,
    }),
    jW = S({ projectId: H, deletedAt: B }),
    MW = {
      sequence: R,
      eventId: H_,
      aggregateKind: _(`project`),
      aggregateId: H,
      occurredAt: B,
      commandId: D(W),
      causationEventId: D(H_),
      correlationId: D(W),
      metadata: OW,
    };
  T([
    S({ ...MW, type: _(`project.created`), payload: kW }),
    S({ ...MW, type: _(`project.meta-updated`), payload: AW }),
    S({ ...MW, type: _(`project.deleted`), payload: jW }),
  ]);
  var NW = L.check(Mg(/^([01]?\d|2[0-3]):([0-5]\d)$/)).annotate({
      description: `Local wall-clock time in 24-hour HH:MM form, such as 09:30.`,
    }),
    PW = 6e4,
    FW = A.check(Vg(0)).annotate({ description: `Positive interval in milliseconds.` }),
    IW = S({
      type: _(`interval`).annotate({ description: `Select interval scheduling.` }),
      everyMs: FW,
    }).annotate({ description: `Run repeatedly after a fixed number of milliseconds.` }),
    LW = S({
      type: _(`fixed_time`).annotate({ description: `Select a fixed local wall-clock time.` }),
      timeOfDay: NW,
      weekdays: g(
        w(
          A.check(Gg({ minimum: 0, maximum: 6 })).annotate({
            description: `Weekday number where 0 is Sunday and 6 is Saturday.`,
          }),
        ).annotate({ description: `Optional weekdays; omit to run every day.` }),
      ),
    }).annotate({ description: `Run at a fixed local wall-clock time on selected weekdays.` }),
    RW = {
      header: L.annotate({
        description: `Request header carrying the signature, such as x-hub-signature-256.`,
      }),
      encoding: E([`hex`, `base64`]).annotate({
        description: `How the HMAC-SHA256 digest is encoded in the header.`,
      }),
      prefix: y.annotate({
        description: `Text before the digest in the header value, such as 'sha256='. Empty for none.`,
      }),
    },
    zW = S(RW).annotate({
      description: `Optional HMAC-SHA256 signature check over the raw request body.`,
    }),
    BW = A.check(Gg({ minimum: 1, maximum: 1440 })).annotate({
      description: `Skip requests the relay held longer than this many minutes while the environment was offline. Null runs every request.`,
    }),
    VW = S({
      type: _(`webhook`).annotate({
        description: `Run when the task's webhook URL receives a request.`,
      }),
      signature: D(zW),
      maxDeliveryAgeMinutes: g(D(BW)),
    }).annotate({
      description: `Run on each request to the task's webhook URL. The prompt may use {{body.path}}, {{headers.name}}, {{query.name}}, {{body}} and {{request}} placeholders.`,
    }),
    HW = S({
      type: _(`webhook`).annotate({
        description: `Run when the task's webhook URL receives a request.`,
      }),
      signature: g(
        D(
          S({
            ...RW,
            secret: g(L).annotate({
              description: `Shared signing secret. Omit to keep the stored secret.`,
            }),
            secretRef: g(av).annotate({
              description: `A secret the user entered through request_secret, used instead of secret. It is consumed by this save.`,
            }),
          }),
        ),
      ).annotate({
        description: `Signature check; omit or null to accept requests by URL token only.`,
      }),
      maxDeliveryAgeMinutes: g(D(BW)),
    }).annotate({
      description: `Run on each request to the task's webhook URL. The prompt may use {{body.path}}, {{headers.name}}, {{query.name}}, {{body}} and {{request}} placeholders.`,
    }),
    UW = T([IW, LW, VW]).annotate({
      description: `Structured trigger. Pass an object with type 'interval', 'fixed_time' or 'webhook'.`,
    }),
    WW = T([
      S({
        type: _(`interval`).annotate({ description: `Select interval scheduling.` }),
        everyMs: FW.check(Hg(PW)).annotate({
          description: `Interval in milliseconds, with a minimum of 60000 (one minute).`,
        }),
      }).annotate({ description: `Run repeatedly after a fixed number of milliseconds.` }),
      LW,
      HW,
    ]).annotate({
      description: `Writable trigger. Pass an object with type 'interval', 'fixed_time' or 'webhook'.`,
    }),
    GW = E([`never`, `running`, `succeeded`, `failed`]),
    KW = S({ path: L, url: D(L), hasSecret: x }),
    qW = S({
      id: iv,
      title: L,
      prompt: L,
      enabled: x,
      schedule: UW,
      projectId: H,
      threadId: D(V),
      workspaceStrategy: QA,
      modelSelection: bw,
      runtimeMode: VE,
      interactionMode: UE,
      createdBy: yA,
      creationSource: bA,
      createdAt: B,
      updatedAt: B,
      nextRunAt: D(B),
      lastRunAt: D(B),
      lastRunStatus: GW,
      lastRunError: D(y),
      runCount: A.check(Hg(0)),
      webhook: g(KW),
    }),
    JW = S({}),
    YW = S({ tasks: P_(qW) }),
    XW = S({
      id: g(iv),
      requireExisting: g(x).annotate({
        description: `Reject the save if the task no longer exists, for edits from a client form.`,
      }),
      commandId: g(W),
      title: L,
      prompt: L,
      enabled: x,
      schedule: WW,
      projectId: H,
      threadId: g(D(V)),
      workspaceStrategy: QA,
      modelSelection: bw,
      runtimeMode: VE,
      interactionMode: UE,
      createdBy: g(yA),
      creationSource: g(bA),
    }),
    ZW = S({ id: iv, enabled: x }),
    QW = S({ id: iv }),
    $W = S({ id: iv }),
    eG = S({ id: iv }),
    tG = L.pipe(mg(`ScheduledTaskWebhookDeliveryId`)),
    nG = E([
      `accepted`,
      `dispatch_failed`,
      `rejected_signature`,
      `disabled`,
      `rate_limited`,
      `expired`,
    ]),
    rG = S({
      id: tG,
      taskId: iv,
      receivedAt: B,
      method: L,
      contentType: D(y),
      bodyBytes: A.check(Hg(0)),
      outcome: nG,
      signatureVerified: x,
      missingFields: w(y),
      error: D(y),
    }),
    iG = S({
      ...rG.fields,
      query: y,
      headers: C(y, y),
      body: y,
      bodyTruncated: x,
      renderedPrompt: D(y),
    }),
    aG = S({ id: iv }),
    oG = S({ deliveries: w(rG) }),
    sG = S({ id: iv, deliveryId: tG }),
    cG = S({ delivery: iG }),
    lG = S({ task: qW }),
    uG = S({ id: iv }),
    dG = S({ task: qW }),
    fG = class extends F()(`ScheduledTaskError`, { message: y, taskId: g(iv), cause: g(M()) }) {},
    pG = S({ requestId: W, startedAt: B }),
    mG = L.check(j(12e4)).annotate({
      description: `Complete task or message text for the target agent.`,
    }),
    hG = L.check(j(512)).annotate({ description: `Optional concise display title.` }),
    gG = L.check(j(256)).annotate({
      description: `Stable idempotency key to reuse when retrying this mutation.`,
    }),
    _G = T([
      WW,
      r_(WW).annotate({
        description: `Compatibility-only JSON encoding of the schedule object. Prefer a structured object.`,
      }),
    ]).annotate({
      description: `Trigger object: {type:'interval', everyMs}, {type:'fixed_time', timeOfDay, weekdays?}, or {type:'webhook'} to run on each request to a generated URL. Never stringify it unless the provider requires the compatibility form.`,
    }),
    vG = C(y, _y).pipe(
      xg(
        w(vy),
        Td({
          decode: (e) => m(Object.entries(e).map(([e, t]) => ({ id: e, value: t }))),
          encode: (e) => m(Object.fromEntries(e.map((e) => [e.id, e.value]))),
        }),
      ),
    ),
    yG = T([w(vy), vG]),
    bG = S({
      providerInstanceId: g(
        q.annotate({
          description: `Configured provider instance id from orchestrator_capabilities.`,
        }),
      ),
      driverKind: g(
        ny.annotate({
          description: `Provider driver kind; prefer providerInstanceId when available.`,
        }),
      ),
      model: g(
        L.annotate({ description: `Model id advertised for the selected provider instance.` }),
      ),
      options: g(
        yG.annotate({
          description: `Model option selections advertised by orchestrator_capabilities.`,
        }),
      ),
    }),
    xG = T([_(`inherit`), VE]),
    SG = T([_(`inherit`), UE]),
    CG = E([`implementation`, `research`, `review`, `design`, `test`, `general`]),
    wG = E([`queued`, `running`, `waiting`, `completed`, `failed`, `cancelled`, `interrupted`]),
    TG = E([`completed`, `failed`, `cancelled`, `interrupted`]);
  (S({
    task: mG.annotate({
      description: `Self-contained task for one delegated child agent/subagent.`,
    }),
    target: g(bG),
    title: g(hG),
    role: g(CG),
    mode: g(
      E([`async`, `wait`]).annotate({
        description: `Defaults to async. Use wait only when this turn needs the child's result before you can continue.`,
      }),
    ),
    timeoutMs: g(b).annotate({
      description: `Wait budget for mode=wait only. Default 10 minutes. Elapsing it returns waitTimedOut=true on that call and does not cancel the child.`,
    }),
    clientRequestId: g(gG),
    runtimeMode: g(xG),
    interactionMode: g(SG),
  }),
    S({
      taskId: K,
      childThreadId: V,
      childRunId: D(G),
      childNodeId: K,
      status: wG,
      workState: E([`working`, `waiting_for_children`, `result_available`]),
      hasPendingChildRuns: x,
      latestTerminalRunId: D(G),
      latestTerminalStatus: D(TG),
      latestTerminalSummary: D(y),
      latestTerminalResultContextTransferId: D(dv),
      providerInstanceId: q,
      model: D(y),
      summary: D(y),
      resultContextTransferId: D(dv),
      waitTimedOut: x.annotate({
        description: `True only on that mode=wait call when timeoutMs elapsed. The timeout does not cancel the child. Later task_status reads return false and use status for liveness.`,
      }),
    }),
    S({ taskId: K }),
    S({ taskId: K, reason: g(y.check(j(2e3))), clientRequestId: g(gG) }),
    S({
      taskId: K,
      status: E([`cancel_requested`, `completed`, `failed`, `cancelled`, `interrupted`]),
    }),
    S({
      threads: w(
        S({
          prompt: g(mG),
          title: g(hG),
          target: g(bG),
          runtimeMode: g(xG),
          interactionMode: g(SG),
        }),
      ).check(qg(1), j(20)),
      clientRequestId: g(gG),
    }));
  var EG = T([_(`idle`), _(`preparing`), _(`starting`), wG, _(`rolled_back`)]);
  S({
    threads: w(
      S({
        threadId: V,
        runId: D(G),
        status: EG,
        title: y,
        createdBy: yA,
        creationSource: bA,
        providerInstanceId: q,
        model: y,
      }),
    ),
  });
  var DG = T([_(`idle`), KA]),
    OG = g(
      H.annotate({
        description: `Project to act on. Omit for the calling thread's project; required when the caller is not a T3 thread.`,
      }),
    );
  S({
    projectId: OG,
    statuses: g(w(DG).check(j(10))),
    titleContains: g(L.check(j(256))),
    settled: g(x),
    snoozed: g(x),
    includeSubagents: g(x),
    cursor: g(R),
    limit: g(z.check(Wg(100))),
  });
  var kG = S({
    threadId: V,
    title: y,
    createdBy: yA,
    creationSource: bA,
    status: DG,
    latestRunId: D(G),
    providerInstanceId: q,
    model: y,
    runtimeMode: VE,
    interactionMode: UE,
    linkedPullRequest: D(jE),
    settled: x,
    settledAt: D(B),
    snoozed: x,
    snoozedUntil: D(B),
    parentThreadId: D(V),
    relationshipToParent: D(E([`fork`, `subagent`])),
    itemCount: R,
    createdAt: B,
    updatedAt: B,
  });
  (S({ projectId: H, currentThreadId: D(V), threads: w(kG), nextCursor: D(R), total: R }),
    S({
      threadId: V,
      itemId: g(tv),
      textOffset: g(R),
      view: g(E([`messages`, `activity`])),
      afterPosition: g(R),
      limit: g(z.check(Wg(100))),
      runLimit: g(z.check(Wg(50))),
      maxCharsPerItem: g(z.check(Wg(5e4))),
    }));
  var AG = S({
      threadId: V,
      projectId: H,
      title: y,
      createdBy: yA,
      creationSource: bA,
      status: DG,
      latestRunId: D(G),
      activeRunId: D(G),
      providerInstanceId: q,
      model: y,
      runtimeMode: VE,
      interactionMode: UE,
      linkedPullRequest: D(jE),
      titleRegeneration: D(pG),
      branch: D(y),
      worktreePath: D(y),
      parentThreadId: D(V),
      relationshipToParent: D(E([`fork`, `subagent`])),
      runCount: R,
      itemCount: R,
      pendingRequestCount: R,
      archived: x,
      settled: x,
      settledAt: D(B),
      snoozed: x,
      snoozedUntil: D(B),
      createdAt: B,
      updatedAt: B,
    }),
    jG = S({
      runId: G,
      ordinal: z,
      status: KA,
      providerInstanceId: q,
      model: y,
      requestedAt: B,
      startedAt: D(B),
      completedAt: D(B),
    }),
    MG = S({
      position: R,
      visibility: E([`local`, `inherited`, `synthetic`]),
      sourceThreadId: V,
      itemId: tv,
      subagentId: g(K),
      runId: D(G),
      messageId: D(U_),
      createdBy: D(yA),
      creationSource: D(bA),
      type: y,
      status: Nj,
      title: D(y),
      text: D(y),
      textTruncated: x,
      nextTextOffset: g(D(R)),
      updatedAt: B,
    });
  (S({ thread: AG, recentRuns: w(jG), items: w(MG), nextPosition: D(R), hasMore: x }),
    S({
      threadId: V,
      message: mG,
      mode: g(E([`auto`, `queue`, `steer`, `restart`])),
      clientRequestId: g(gG),
    }),
    S({
      threadId: V,
      messageId: U_,
      runId: G,
      taskId: g(K),
      status: KA,
      delivery: E([`started`, `queued`, `steered`, `restarted`]),
    }),
    S({ threadId: V, runId: g(G), timeoutMs: g(b) }),
    S({ threadId: V, runId: D(G), status: DG, timedOut: x }),
    S({ threadId: V, runId: g(G), reason: g(y.check(j(2e3))), clientRequestId: g(gG) }),
    S({
      threadId: V,
      runId: D(G),
      status: T([
        _(`interrupt_requested`),
        _(`no_active_run`),
        E([`completed`, `failed`, `cancelled`, `interrupted`, `rolled_back`]),
      ]),
    }));
  var NG = S({
    providerInstanceId: q,
    driverKind: ny,
    displayName: D(y),
    models: w(S({ id: y, label: D(y), options: g(w(gy)) })),
    runtimeModes: g(w(VE)),
    canRunChildTask: x,
    canRunCrossProviderChildTask: x,
    constraints: w(y),
  });
  (S({
    parentThreadId: D(V),
    inheritedProviderInstanceId: D(q),
    inheritedModel: D(y),
    runtimeMode: VE,
    interactionMode: UE,
    providers: w(NG),
    features: S({
      appOwnedSubagents: x,
      asyncPolling: x,
      cancellation: x,
      batchThreadCreation: x,
      threadManagement: x,
      incrementalThreadRead: x,
      scheduledTasks: x,
      maxBatchThreads: b,
    }),
  }),
    S({
      projectId: OG,
      prompt: mG.annotate({ description: `Prompt executed on every scheduled run.` }),
      schedule: _G,
      title: g(hG),
      enabled: g(
        x.annotate({ description: `Whether the schedule starts enabled; defaults true.` }),
      ),
      bindToCurrentThread: g(
        x.annotate({
          description: `True (default) posts each run into this thread; false creates a fresh top-level thread per run.`,
        }),
      ),
      clientRequestId: g(gG),
    }));
  var PG = S({
    scheduledTaskId: iv,
    title: y,
    prompt: y,
    enabled: x,
    projectId: H,
    boundThreadId: D(V),
    schedule: UW,
    nextRunAt: D(B),
    lastRunStatus: GW,
    webhookUrl: g(y).annotate({
      description: `Public URL to give the sender. Absent when this environment has no T3 Connect managed tunnel; the user must enable T3 Connect remote access first.`,
    }),
    webhookSignature: g(E([`none`, `set`])).annotate({
      description: `Whether requests must carry a valid signature.`,
    }),
  });
  (S({
    projectId: g(
      H.annotate({
        description: `Only list this project's tasks. Omit for the calling thread's project, or for every project when the caller is not a T3 thread.`,
      }),
    ),
  }),
    S({ tasks: w(PG) }),
    S({
      scheduledTaskId: iv,
      prompt: g(mG),
      title: g(hG),
      schedule: g(_G),
      enabled: g(x),
      bindToCurrentThread: g(x),
    }),
    S({
      label: L.annotate({
        description: `What you need, shown as the card's title, e.g. 'GitHub webhook secret'.`,
      }),
      reason: L.annotate({
        description: `One or two sentences on what it is for and where the user gets or also enters it.`,
      }),
      placeholder: g(L).annotate({
        description: `Hint inside the input, e.g. 'Paste your GitHub token'.`,
      }),
      timeoutMs: g(A.check(Gg({ minimum: 1e3, maximum: 36e5 }))).annotate({
        description: `How long to wait for the user. Default 10 minutes.`,
      }),
      clientRequestId: g(gG).annotate({
        description: `Reuse when retrying a call that lost its result, so the user sees one card and its answer is returned again. Use a new id to ask again after timed_out or cancelled.`,
      }),
    }),
    T([
      S({
        status: _(`saved`).annotate({ description: `secretRef holds the value.` }),
        secretRef: av.annotate({
          description: `Pass it to a tool that accepts a secretRef; it works once, and you never see the value.`,
        }),
      }),
      S({
        status: E([`declined`, `cancelled`, `timed_out`]).annotate({
          description: `declined: the user chose not to. cancelled: the request ended with the run. timed_out: the user did not answer in time; the card is closed, so ask again with a new clientRequestId if still needed.`,
        }),
      }),
    ]),
    S({ scheduledTaskId: iv }),
    S({ scheduledTaskId: iv, deleted: x }),
    F()(`OrchestratorMcpFailure`, {
      code: E([
        `capability_denied`,
        `parent_not_active`,
        `provider_unavailable`,
        `model_unavailable`,
        `runtime_mode_escalation_denied`,
        `interaction_mode_escalation_denied`,
        `task_not_found`,
        `task_not_cancellable`,
        `thread_not_found`,
        `run_not_found`,
        `thread_not_sendable`,
        `thread_not_interruptible`,
        `invalid_request`,
        `orchestration_error`,
        `thread_credential_required`,
        `target_required`,
      ]),
      message: y,
    }));
  function FG(e) {
    for (let t = 0; t < e.length; t += 1) {
      let n = e.charCodeAt(t);
      if (n >= 55296 && n <= 56319) {
        let n = e.charCodeAt(t + 1);
        if (t + 1 >= e.length || n < 56320 || n > 57343) return !1;
        t += 1;
      } else if (n >= 56320 && n <= 57343) return !1;
    }
    return !0;
  }
  function IG(e) {
    try {
      let t = new URL(e);
      return t.protocol === `http:` || t.protocol === `https:`;
    } catch {
      return !1;
    }
  }
  var LG = L.check(j(512)).annotate({
      description: `New concise display title. Required only when action is rename.`,
    }),
    RG = L.check(j(256))
      .check(
        k((e) => (FG(e) ? !0 : `clientRequestId must contain valid paired Unicode surrogates.`)),
      )
      .annotate({ description: `Stable idempotency key to reuse when retrying this mutation.` }),
    zG = L.check(
      k((e) => (IG(e) ? !0 : `Pull request URL must be a well-formed HTTP(S) URL.`)),
    ).annotate({
      description: `Canonical HTTP(S) pull request URL, including self-hosted repository URLs.`,
    }),
    BG = E([`rename`, `regenerate_title`, `link_pull_request`, `unlink_pull_request`]).annotate({
      description: `Metadata mutation: rename, regenerate_title, link_pull_request, or unlink_pull_request.`,
    }),
    VG = S({
      repository: L.annotate({ description: `Repository name as owner/name.` }),
      number: z.annotate({ description: `Pull request number.` }),
      url: zG,
    });
  (S({
    threadId: g(V).annotate({
      description: `Thread to update. Omit to update the calling thread.`,
    }),
    action: BG,
    title: g(LG),
    pullRequest: g(VG).annotate({
      description: `Pull request to link. Required only when action is link_pull_request.`,
    }),
    clientRequestId: g(RG),
  }).check(
    k((e) => {
      switch (e.action) {
        case `rename`:
          return (
            (e.title !== void 0 && e.pullRequest === void 0) ||
            `rename requires title and does not accept pullRequest.`
          );
        case `link_pull_request`:
          return (
            (e.pullRequest !== void 0 && e.title === void 0) ||
            `link_pull_request requires pullRequest and does not accept title.`
          );
        case `regenerate_title`:
        case `unlink_pull_request`:
          return (
            (e.title === void 0 && e.pullRequest === void 0) ||
            `${e.action} does not accept title or pullRequest.`
          );
      }
    }),
  ),
    S({
      threadId: V,
      action: BG,
      commandId: W,
      sequence: R,
      title: y,
      titleRegeneration: D(pG),
      linkedPullRequest: D(jE),
      updatedAt: B,
    }));
  var HG = E([`user`, `assistant`]),
    UG = S({ query: I.check(qg(2), j(200)), limit: h(A.check(Gg({ minimum: 1, maximum: 50 }))) }),
    WG = S({
      matches: w(
        S({
          threadId: V,
          projectId: H,
          source: HG,
          snippet: y.check(j(240)),
          messageCreatedAt: D(B),
        }),
      ),
    }),
    GG = class extends F()(`OrchestrationSearchThreadsError`, { message: L, cause: g(M()) }) {},
    KG = `https://t3.codes/schema/t3.json`,
    qG = 512,
    JG = 50,
    YG = (e, t) => {
      let n = y.annotate(e),
        r = t === void 0 ? n.check(Yg()) : n.check(Yg(), j(t));
      return r.pipe(xg(r, Od()));
    },
    XG = S({
      name: YG({ description: `Display name for the script, shown in the S5 Code scripts menu.` }),
      command: YG({
        description: `Shell command executed in a S5 Code terminal at the project root.`,
      }),
      icon: h(
        eD.annotate({
          description: `Icon shown next to the script in the scripts menu. Defaults to "play".`,
        }),
      ),
      runOnWorktreeCreate: h(
        x.annotate({
          description: `When true, the script runs automatically after a worktree is created for a new thread.`,
        }),
      ),
      runOnSettle: h(
        x.annotate({
          description: `When true, the script runs in the thread's worktree each time the thread settles, for example to delete build output. Threads without their own worktree skip it.`,
        }),
      ),
      async: h(
        x.annotate({
          description: `Only for runOnWorktreeCreate scripts. When true (the default), the agent starts while the script is still running. Set false to hold the agent until the script exits.`,
        }),
      ),
      previewUrl: h(
        YG({
          description: `URL opened in the in-app browser preview when this script runs. Only honored on the desktop build.`,
        }),
      ),
      autoOpenPreview: h(
        x.annotate({
          description:
            "When true, automatically open the preview panel at `previewUrl` the moment the script starts.",
        }),
      ),
    }).annotate({ description: `A project script that team members can import into S5 Code.` });
  S({
    $schema: h(
      y.annotate({ description: `URL of the JSON Schema for this file, typically "${KG}".` }),
    ),
    iconPath: h(
      YG(
        {
          description: `Workspace-relative path to the project icon (e.g. "assets/logo.svg"). Checked before S5 Code's built-in icon locations.`,
        },
        qG,
      ),
    ),
    defaultThreadEnvMode: h(
      aS.annotate({
        description: `Where new threads start for this repository: "worktree" for a fresh git worktree, "local" for the current checkout. A per-project setting in T3 Code overrides this; when neither is set, the global default applies.`,
      }),
    ),
    worktreeSubmodules: h(
      oS.annotate({
        description: `How new worktrees populate git submodules: "recursive" (the default) initializes nested submodules too, "top-level" initializes only those declared by this repository, and "none" leaves every submodule empty for a setup script to handle. A project or environment setting in T3 Code overrides this.`,
      }),
    ),
    scripts: h(
      w(XG)
        .annotate({
          description: `Project scripts shared with everyone who opens this repository in S5 Code.`,
        })
        .check(j(JG)),
    ),
  }).annotate({
    title: `T3 project file`,
    description: `Checked-in project configuration for S5 Code (t3.json at the repository root). See https://t3.codes for documentation.`,
  });
  var ZG = 512,
    QG = S({ paths: w(L.check(j(ZG))).check(qg(1), j(64)) }),
    $G = S({
      entries: w(
        D(S({ kind: E([`file`, `directory`, `other`]), byteLength: h(R), mimeType: h(L) })),
      ),
    }),
    eK = S({ partialPath: L.check(j(ZG)), cwd: g(L.check(j(ZG))) }),
    tK = S({ parentPath: L, entries: w(S({ name: L, fullPath: L })) }),
    nK = E([`windows_path_unsupported`, `current_project_required`, `read_directory_failed`]);
  function rK(e) {
    if (`message` in e) return typeof e.message == `string` ? e.message : void 0;
  }
  var iK = class extends F()(`FilesystemBrowseError`, {
      partialPath: g(L),
      cwd: g(L),
      failure: g(nK),
      parentPath: g(L),
      platform: g(L),
      message: L,
      cause: g(M()),
    }) {
      constructor(e) {
        let t = e.cwd === void 0 ? `` : ` from '${e.cwd}'`;
        super({
          ...e,
          message: rK(e) ?? `Failed to browse filesystem path '${e.partialPath}'${t}.`,
        });
      }
    },
    aK = E([`claudeAgent`, `codex`]);
  S({
    provider: aK,
    providerInstanceId: q,
    providerSessionId: L,
    filePath: L,
    size: R,
    mtimeMs: D(b),
    device: b,
    inode: D(b),
    birthtimeMs: D(b),
  });
  var oK = S({}),
    sK = S({ remoteKey: D(y), repository: D(y) }),
    cK = S({
      candidates: w(
        S({
          path: L,
          title: L,
          projectId: g(H),
          sources: w(aK),
          threadCount: R,
          lastActiveAt: D(B),
          alreadyImported: x,
          git: h(D(sK)),
        }),
      ),
      scannedAt: B,
      truncated: g(x),
    }),
    lK = S({ projectId: H, expectedWorkspaceRoot: g(L) }),
    uK = class extends F()(`AgentSessionImportProjectNotFoundError`, { projectId: H }) {
      get message() {
        return `Project '${this.projectId}' does not exist.`;
      }
    },
    dK = class extends F()(`AgentSessionImportProjectChangedError`, { projectId: H }) {
      get message() {
        return `Project '${this.projectId}' changed directories. Scan for projects again before importing history.`;
      }
    },
    fK = S({ importedCount: R, skippedCount: R }),
    pK = class extends F()(`AgentSessionScanError`, {
      operation: E([`read-settings`, `read-projects`]),
      cause: M(),
    }) {
      get message() {
        return `Failed to scan agent sessions during ${this.operation}.`;
      }
    },
    mK = 1024,
    hK = T([
      Dg(`workspace-file`, { threadId: V, path: L.check(j(mK)) }),
      Dg(`media-file`, { threadId: V, path: L.check(j(mK)) }),
      Dg(`draft-workspace-file`, { cwd: L.check(j(mK)), path: L.check(j(mK)) }),
      Dg(`attachment`, {
        attachmentId: L.check(j(256)),
        fileName: h(L.check(j(255))),
        mimeType: h(L.check(j(100))),
        disposition: h(E([`inline`, `attachment`])),
      }),
      Dg(`tool-output-image`, { threadId: V, itemId: tv, index: R }),
      Dg(`project-favicon`, { cwd: L.check(j(mK)), path: g(lD) }),
      Dg(`native-app-icon`, { app: Rk }),
      Dg(`github-media`, { cwd: L.check(j(mK)), url: L.check(j(2048)) }),
    ]),
    gK = S({ resource: hK }),
    _K = S({ width: R.check(Hg(1)), height: R.check(Hg(1)) }),
    vK = S({
      relativeUrl: L.check(j(4096)),
      expiresAt: b,
      sourcePath: g(L.check(j(mK))),
      imageDimensions: g(_K),
    }),
    yK = T([
      S({
        type: h(_(`image`)),
        name: L.check(j(255)),
        mimeType: E(UC),
        sizeBytes: R.check(Hg(1), Wg(BC)),
      }),
      S({
        type: _(`file`),
        name: L.check(j(255)),
        mimeType: L.check(j(100)),
        sizeBytes: R.check(Hg(1), Wg(HC)),
      }),
    ]),
    bK = S({ attachmentId: L.check(j(256)), relativeUrl: L.check(j(4096)), expiresAt: b }),
    xK = S({ attachmentId: L.check(j(256)) }),
    SK = class extends F()(`AttachmentUploadSigningKeyError`, { cause: M() }) {
      get message() {
        return `Failed to load the attachment upload signing key.`;
      }
    },
    CK = class extends F()(`AssetWorkspaceContextNotFoundError`, { resource: hK }) {
      get message() {
        return `Workspace context was not found.`;
      }
    },
    wK = class extends F()(`AssetWorkspaceContextResolutionError`, { resource: hK, cause: M() }) {
      get message() {
        return `Failed to resolve workspace context.`;
      }
    },
    TK = class extends F()(`AssetWorkspaceRootNormalizationError`, { resource: hK, cause: M() }) {
      get message() {
        return `Failed to normalize the workspace root.`;
      }
    },
    EK = class extends F()(`AssetWorkspacePathValidationError`, { resource: hK, cause: M() }) {
      get message() {
        return `Workspace file path must be relative to the project root.`;
      }
    },
    DK = class extends F()(`AssetPreviewTypeValidationError`, { resource: hK }) {
      get message() {
        return this.resource._tag === `media-file` || this.resource._tag === `draft-workspace-file`
          ? `Only images, videos, audio, HTML, and PDF files can be previewed.`
          : `Only browser documents and images can be previewed.`;
      }
    },
    OK = class extends F()(`AssetWorkspaceAssetInspectionError`, { resource: hK, cause: M() }) {
      get message() {
        return this.resource._tag === `media-file`
          ? `Failed to inspect the media file.`
          : `Failed to inspect the workspace asset.`;
      }
    },
    kK = class extends F()(`AssetWorkspaceAssetNotFoundError`, { resource: hK }) {
      get message() {
        return this.resource._tag === `media-file`
          ? `Media file was not found.`
          : this.resource._tag === `tool-output-image`
            ? `Tool output image was not found.`
            : `Workspace asset was not found.`;
      }
    },
    AK = class extends F()(`AssetWorkspaceResolutionError`, { resource: hK, cause: M() }) {
      get message() {
        return `Failed to resolve workspace.`;
      }
    },
    jK = class extends F()(`AssetAttachmentNotFoundError`, { resource: hK }) {
      get message() {
        return `Attachment was not found.`;
      }
    },
    MK = class extends F()(`AssetProjectFaviconResolutionError`, { resource: hK, cause: M() }) {
      get message() {
        return `Failed to resolve project favicon.`;
      }
    },
    NK = class extends F()(`AssetProjectFaviconInspectionError`, { resource: hK, cause: M() }) {
      get message() {
        return `Failed to inspect the project favicon.`;
      }
    },
    PK = class extends F()(`AssetProjectFaviconNotFoundError`, { resource: hK }) {
      get message() {
        return `Project favicon was not found.`;
      }
    },
    FK = class extends F()(`AssetSigningKeyLoadError`, { resource: hK, cause: M() }) {
      get message() {
        return `Failed to load the asset signing key.`;
      }
    },
    IK = T([
      CK,
      wK,
      TK,
      EK,
      DK,
      OK,
      kK,
      AK,
      jK,
      MK,
      NK,
      PK,
      class extends F()(`AssetGitHubMediaUrlValidationError`, {}) {
        get message() {
          return `Only media hosted by GitHub can be fetched with a GitHub credential.`;
        }
      },
      FK,
    ]),
    LK = S({
      cwd: L,
      baseRef: g(L),
      ignoreWhitespace: h(x),
      file: h(
        S({ path: Qg, previousPath: D(Qg), sourceKind: E([`working-tree`, `branch-range`]) }),
      ),
    }),
    RK = E([`working-tree`, `branch-range`]),
    zK = S({ path: y, previousPath: D(y), additions: b, deletions: b }),
    BK = S({
      id: L,
      kind: RK,
      title: L,
      baseRef: D(L),
      headRef: D(L),
      diff: y,
      diffHash: L,
      truncated: x,
      files: h(w(zK)),
    }),
    VK = S({
      cwd: L,
      sourceKind: RK,
      changeType: E([`change`, `rename-pure`, `rename-changed`, `new`, `deleted`]),
      baseRef: D(L),
      headRef: D(L),
      oldPath: L,
      newPath: L,
    }),
    HK = S({ oldContents: y, newContents: y }),
    UK = S({ cwd: L, generatedAt: N, sources: w(BK) }),
    WK = T([Mw, aW]),
    GK = { threadId: V, itemId: tv },
    KK = S({ ...GK, name: L, arguments: C(y, v) }),
    qK = S({ content: w(v), structuredContent: g(v), isError: g(x), _meta: g(v) }),
    JK = S({ ...GK, name: L }),
    YK = S({ callable: x, readOnly: x, title: g(y), tool: g(v) }),
    XK = S({ ...GK, uri: L }),
    ZK = S({ contents: w(v) }),
    QK = S({ ...GK, conversationThreadId: V, content: g(w(v)), structuredContent: g(C(y, v)) }),
    $K = E([
      `not-an-app`,
      `provider-unsupported`,
      `session-stopped`,
      `tool-not-callable`,
      `unsupported-content`,
      `request-failed`,
    ]),
    eq = class extends F()(`McpAppRequestError`, { threadId: V, reason: $K, cause: g(M()) }) {
      get message() {
        switch (this.reason) {
          case `not-an-app`:
            return `This tool call has no MCP app.`;
          case `provider-unsupported`:
            return `This thread's provider cannot run MCP app requests.`;
          case `session-stopped`:
            return `The app's thread is not running. Send a message in the thread that created it to use the app again.`;
          case `tool-not-callable`:
            return `This app cannot call that tool.`;
          case `unsupported-content`:
            return `Only text and structured content are supported.`;
          case `request-failed`:
            return `The app's MCP server request failed.`;
        }
      }
    },
    tq = E([`chrome`, `edge`, `brave`, `vivaldi`, `opera`, `arc`, `helium`, `firefox`, `safari`]),
    nq = E([
      `notInstalled`,
      `needsKeychainApproval`,
      `keychainItemMissing`,
      `needsFullDiskAccess`,
      `browserRunning`,
      `unsupportedPlatform`,
    ]);
  (E([
    ...nq.literals,
    `keychainUnavailable`,
    `unknownSource`,
    `unknownSourceProfile`,
    `sessionUnavailable`,
    `profileNotSaved`,
    `profileLimitReached`,
    `readFailed`,
  ]),
    S({
      id: tq,
      name: L,
      profiles: w(S({ directory: L, name: L, cookieCount: g(A) })),
      unavailable: g(nq),
    }),
    S({ sourceId: tq, sourceProfileDirectory: L, targetProfileId: kI }),
    S({ environmentId: L, sourceId: tq, sourceProfileDirectory: L, targetProfileId: kI }),
    S({ imported: A, skipped: A, skippedDomains: w(y) }));
  var rq = y.check(jg()).check(Yg()).check(j(2048)),
    iq = `Absolute http(s) URL or a schemeless host such as t3.chat or localhost:5173. Schemeless public hosts use https; loopback hosts use http.`,
    aq = `Browser profile for a new tab, by id or name (case-insensitive) from preview_status profiles. Omit to use the user's default profile. A tab keeps the profile it opened with.`,
    oq = g(
      A.check(Vg(0))
        .check(Wg(6e4))
        .annotate({
          description: `Maximum wait in milliseconds. Defaults to 15000; maximum 60000.`,
        }),
    ).annotate({ description: `Maximum wait in milliseconds. Defaults to 15000; maximum 60000.` }),
    sq = E([
      `status`,
      `open`,
      `navigate`,
      `snapshot`,
      `click`,
      `type`,
      `press`,
      `scroll`,
      `evaluate`,
      `waitFor`,
      `recordingStart`,
      `recordingStop`,
      `resize`,
      `setColorScheme`,
      `dialog`,
      `close`,
      `upload`,
      `hover`,
      `select`,
      `drag`,
    ]),
    cq = {
      tabId: g(
        AL.annotate({
          description: `Exact collaborative browser tab to target, including a tab the user opened (see preview_status tabs). Omit to use this agent session's current tab, or the user's visible tab when this session has none.`,
        }),
      ).annotate({
        description: `Exact collaborative browser tab to target, including a tab the user opened (see preview_status tabs). Omit to use this agent session's current tab, or the user's visible tab when this session has none.`,
      }),
    };
  S(cq);
  var lq = E([`agent`, `human`, `unclaimed`]),
    uq = S({ id: kI, name: y, incognito: g(x) });
  (S({
    available: x,
    visible: x,
    tabId: D(AL),
    url: D(y),
    title: D(y),
    loading: x,
    control: g(S({ owner: lq, ownedByCaller: x, generation: b })),
    dialog: g(D(S({ type: y, message: y, defaultValue: y }))),
    viewportSetting: g(VL),
    viewport: g(IL),
    fileChooser: g(D(S({ multiple: x, accept: y }))),
    downloads: g(w(S({ fileName: y, path: y, sizeBytes: b, url: y, completedAt: y }))),
    tabs: g(
      w(
        S({
          tabId: AL,
          url: D(y),
          openerTabId: g(AL),
          owner: g(lq),
          ownedByCaller: g(x),
          visible: g(x),
          profileId: g(kI),
        }),
      ),
    ),
    profiles: g(w(uq)),
    defaultProfileId: g(kI),
  }),
    S({
      ...cq,
      accept: x.annotate({
        description: `Accept the pending browser dialog when true, or dismiss it when false.`,
      }),
      promptText: g(y).annotate({
        description: `Optional text to submit when accepting a prompt dialog.`,
      }),
    }),
    S({
      ...cq,
      url: g(rq).annotate({
        description: `Optional initial page URL. ${iq} Omit to open a blank tab.`,
      }),
      open: g(
        x.annotate({
          description: `Whether to open the thread-bound inline preview for the human. Defaults to true; set false for background-only automation.`,
        }),
      ),
      show: g(
        x.annotate({
          description: `Deprecated alias for open. Whether to reveal the thread-bound inline preview to the human.`,
        }),
      ),
      reuseExistingTab: g(
        x.annotate({
          description: `Reuse tabId when supplied, otherwise this agent session's current tab. Defaults to true; set false to create a new tab.`,
        }),
      ),
      profileId: g(L.check(j(64)).annotate({ description: aq })).annotate({ description: aq }),
    })
      .check(
        k(
          (e) =>
            e.tabId === void 0 ||
            e.reuseExistingTab !== !1 ||
            `tabId cannot be combined with reuseExistingTab=false.`,
        ),
      )
      .annotate({
        description: `Opens the collaborative browser for the current thread. Use preview_navigate afterward when readiness waiting matters.`,
      }));
  var dq = T([
    S({
      kind: _(`url`).annotate({ description: `Selects direct URL navigation.` }),
      url: rq.annotate({ description: `Direct website URL. ${iq}` }),
    }),
    S({
      kind: _(`environment-port`).annotate({
        description: `Selects a dev-server port relative to the current execution environment.`,
      }),
      port: A.check(Vg(0))
        .check(Ug(65536))
        .annotate({ description: `Dev-server TCP port inside the current environment.` }),
      protocol: g(
        E([`http`, `https`]).annotate({ description: `Dev-server protocol. Defaults to http.` }),
      ),
      path: g(
        y.annotate({
          description: `Optional path, query, and fragment, for example /settings?tab=account.`,
        }),
      ),
    }),
  ]);
  (S({
    ...cq,
    url: g(rq).annotate({
      description: `Website URL. ${iq} Use this for public pages and directly reachable URLs.`,
    }),
    target: g(
      dq.annotate({
        description: `Environment-relative target. Prefer {kind:'environment-port',port:5173} for a dev server in the current environment.`,
      }),
    ).annotate({
      description: `Environment-relative target. Prefer {kind:'environment-port',port:5173} for a dev server in the current environment.`,
    }),
    readiness: g(
      E([`load`, `domContentLoaded`, `none`]).annotate({
        description: `Readiness milestone before returning. 'load' waits for loading to stop (default), 'domContentLoaded' waits for an interactive document, and 'none' returns immediately.`,
      }),
    ).annotate({
      description: `Readiness milestone before returning. 'load' is the default; use 'none' only when a later wait call will verify the page.`,
    }),
    timeoutMs: oq,
  })
    .check(
      k(
        (e) =>
          Number(e.url !== void 0) + Number(e.target !== void 0) === 1 ||
          `Provide exactly one of url or target.`,
      ),
    )
    .annotate({
      description: `Navigates the active browser tab. Provide exactly one of url or target; for most public pages use url.`,
    }),
    S({
      ...cq,
      mode: E([`fill`, `freeform`, `preset`]).annotate({
        description: `Viewport mode: fill follows the preview panel, freeform uses exact independently resizable dimensions, and preset uses a named device size.`,
      }),
      preset: g(
        RL.annotate({
          description: `Named viewport from Chrome DevTools' standard device catalog.`,
        }),
      ).annotate({ description: `Named device size. Required only when mode is preset.` }),
      width: g(
        FL.fields.width.annotate({
          description: `Freeform viewport width in CSS pixels. Required only in freeform mode.`,
        }),
      ).annotate({
        description: `Freeform viewport width in CSS pixels. Required only in freeform mode.`,
      }),
      height: g(
        FL.fields.height.annotate({
          description: `Freeform viewport height in CSS pixels. Required only in freeform mode.`,
        }),
      ).annotate({
        description: `Freeform viewport height in CSS pixels. Required only in freeform mode.`,
      }),
      orientation: g(
        E([`portrait`, `landscape`]).annotate({
          description: `Orientation for a fixed device preset. Defaults to the preset's native orientation.`,
        }),
      ).annotate({
        description: `Orientation for a named device preset. It is not accepted in fill or freeform mode.`,
      }),
      timeoutMs: oq,
    })
      .check(
        k((e) => {
          let t = e.preset !== void 0,
            n = e.width !== void 0,
            r = e.height !== void 0;
          if (n !== r) return `Custom dimensions require both width and height.`;
          if (e.mode === `fill`)
            return (
              (!t && !n && e.orientation === void 0) ||
              `Fill mode does not accept a preset, dimensions, or orientation.`
            );
          if (e.mode === `freeform`) {
            if (!n || !r || t || e.orientation !== void 0)
              return `Freeform mode requires width and height and does not accept a preset or orientation.`;
          } else if (!t || n || r)
            return `Preset mode requires a preset and does not accept custom dimensions.`;
          return n && r && e.width * e.height > 8294400
            ? `Custom viewport area must not exceed ${ML} pixels.`
            : !0;
        }),
      )
      .annotate({
        description: `Sets the active browser tab to fill-panel, independently resizable freeform, or named device-preset sizing.`,
      }),
    S({ tabId: AL, setting: VL, viewport: IL }));
  var fq = E([`system`, `light`, `dark`]);
  (S({
    ...cq,
    colorScheme: fq.annotate({
      description: `Emulated prefers-color-scheme for the page: light, dark, or system to follow the OS appearance.`,
    }),
  }).annotate({
    description: `Emulates prefers-color-scheme in the active browser tab without changing the OS or app theme.`,
  }),
    S({ tabId: AL, colorScheme: fq }));
  var pq = L.annotate({
      description: `Use locator='aria-ref=<ref>' with a ref from the latest server snapshot (including iframe elements), or a unique Playwright role/text selector. Refs expire on navigation, a new snapshot, or control handoff; refresh the snapshot after a stale-ref error.`,
    }),
    mq = L.annotate({
      description: `Legacy CSS selector such as button[type='submit']. Prefer locator for resilient role/text targeting.`,
    }),
    hq = {
      ...cq,
      selector: g(mq).annotate({
        description: `Legacy CSS selector such as button[type='submit']. Prefer locator for resilient role/text targeting.`,
      }),
      locator: g(pq).annotate({
        description: `Use locator='aria-ref=<ref>' with a ref from the latest server snapshot (including iframe elements), or a unique Playwright role/text selector. Refs expire on navigation, a new snapshot, or control handoff; refresh the snapshot after a stale-ref error.`,
      }),
      x: g(
        Ng.annotate({
          description: `Viewport-relative X coordinate in CSS pixels. Must be paired with y.`,
        }),
      ),
      y: g(
        Ng.annotate({
          description: `Viewport-relative Y coordinate in CSS pixels. Must be paired with x.`,
        }),
      ),
      timeoutMs: oq,
    },
    gq = k((e) => {
      let t = Number(e.selector !== void 0) + Number(e.locator !== void 0),
        n = e.x !== void 0,
        r = e.y !== void 0;
      return n === r
        ? t + (n && r ? 1 : 0) === 1 || `Provide exactly one target.`
        : `Coordinates require both x and y.`;
    });
  (S({
    ...hq,
    button: g(
      E([`left`, `right`, `middle`]).annotate({
        description: `Mouse button. Defaults to left; right opens the page's context menu.`,
      }),
    ),
    clickCount: g(
      A.check(Gg({ minimum: 1, maximum: 3 })).annotate({
        description: `1 for a click, 2 for a double-click, 3 for a triple-click. Defaults to 1.`,
      }),
    ),
  })
    .check(gq)
    .annotate({
      description: `Clicks one target. Provide exactly one of locator, selector, or the x/y coordinate pair.`,
    }),
    S(hq)
      .check(gq)
      .annotate({
        description: `Moves the mouse over one target. Provide exactly one of locator, selector, or the x/y coordinate pair.`,
      }),
    S({
      ...cq,
      selector: g(mq).annotate({
        description: `Legacy CSS selector for a <select>. Prefer locator.`,
      }),
      locator: g(pq).annotate({
        description: `The <select> element, for example aria-ref=<ref> or role=combobox[name='Size'].`,
      }),
      values: w(y)
        .check(j(100))
        .annotate({
          description: `Option values or visible labels to select. Pass several for a multiple select, or an empty list to clear it.`,
        }),
      timeoutMs: oq,
    })
      .check(
        k(
          (e) =>
            Number(e.selector !== void 0) + Number(e.locator !== void 0) === 1 ||
            `Provide exactly one of selector or locator.`,
        ),
      )
      .annotate({ description: `Chooses options in one native <select> element.` }),
    S({ selected: w(y).annotate({ description: `The values of the options now selected.` }) }));
  var _q = (e) =>
    y
      .check(jg())
      .check(Yg({ description: e }))
      .annotateKey({ description: e });
  (S({
    ...cq,
    source: _q(
      `Locator of the element to drag, for example aria-ref=<ref> from the latest snapshot.`,
    ),
    target: _q(`Locator of the element to drop onto, for example aria-ref=<ref>.`),
    timeoutMs: oq,
  }).annotate({ description: `Drags one element and drops it onto another.` }),
    S({
      ...cq,
      paths: w(L)
        .check(j(20))
        .annotate({
          description: `Absolute paths of files on the environment to give the page. An empty list cancels the open file picker.`,
        }),
      selector: g(mq).annotate({
        description: `Legacy CSS selector for an <input type=file>. Prefer locator.`,
      }),
      locator: g(pq).annotate({
        description: `Set files directly on this <input type=file> instead of answering the open file picker.`,
      }),
      timeoutMs: oq,
    })
      .check(
        k(
          (e) =>
            e.selector === void 0 ||
            e.locator === void 0 ||
            `Provide at most one of selector or locator.`,
        ),
      )
      .annotate({
        description: `Answers the page's open file picker, or sets files on a file input when locator/selector is given.`,
      }),
    S({
      ...cq,
      text: y.annotate({ description: `Literal text to insert.` }),
      selector: g(mq).annotate({
        description: `Legacy CSS selector for the input. Prefer locator.`,
      }),
      locator: g(pq).annotate({
        description: `Playwright selector for the input, for example role=textbox[name='Message'] or textarea[placeholder*='Message'].`,
      }),
      clear: g(
        x.annotate({
          description: `Clear the existing input value before inserting text. Defaults to false.`,
        }),
      ),
      timeoutMs: oq,
    })
      .check(
        k(
          (e) =>
            e.selector === void 0 ||
            e.locator === void 0 ||
            `Provide at most one of selector or locator.`,
        ),
      )
      .annotate({
        description: `Types into locator/selector, or into the currently focused element when neither target is provided.`,
      }),
    S({
      ...cq,
      key: y
        .check(jg())
        .check(
          Yg({
            description: `Keyboard key name such as Enter, Escape, Tab, ArrowDown, Backspace, or a single character.`,
          }),
        )
        .annotateKey({
          description: `Keyboard key name such as Enter, Escape, Tab, ArrowDown, Backspace, or a single character.`,
        }),
      modifiers: g(
        w(E([`Alt`, `Control`, `Meta`, `Shift`])).annotate({
          description: `Modifier keys held while pressing key.`,
        }),
      ),
    }).annotate({ description: `Presses one keyboard key in the active browser tab.` }),
    S({
      ...cq,
      deltaX: g(
        Ng.annotate({
          description: `Horizontal scroll delta in CSS pixels. Positive scrolls right. Defaults to 0.`,
        }),
      ),
      deltaY: g(
        Ng.annotate({
          description: `Vertical scroll delta in CSS pixels. Positive scrolls down. Defaults to 0.`,
        }),
      ),
      selector: g(mq).annotate({
        description: `Legacy CSS selector for a scrollable container. Omit to scroll the viewport.`,
      }),
      locator: g(pq).annotate({
        description: `Playwright selector for a scrollable container. Omit to scroll the viewport.`,
      }),
    })
      .check(
        k((e) =>
          e.selector !== void 0 && e.locator !== void 0
            ? `Provide at most one of selector or locator.`
            : e.deltaX !== void 0 || e.deltaY !== void 0 || `Provide deltaX or deltaY.`,
        ),
      )
      .annotate({
        description: `Scrolls the viewport, or a locator/selector container. Provide deltaX, deltaY, or both.`,
      }),
    S({
      ...cq,
      expression: y
        .check(jg())
        .check(
          Yg({
            description: `JavaScript expression evaluated in the page's main frame, for example document.title or (() => ({href: location.href}))().`,
          }),
        )
        .check(j(64e3))
        .annotateKey({
          description: `JavaScript expression evaluated in the page's main frame, for example document.title or (() => ({href: location.href}))().`,
        }),
      awaitPromise: g(x.annotate({ description: `Await a returned Promise. Defaults to true.` })),
      returnByValue: g(
        x.annotate({
          description: `Serialize and return the value instead of a remote object reference. Defaults to true.`,
        }),
      ),
    }).annotate({
      description: `Evaluates JavaScript in the page. Prefer snapshot and semantic actions; use evaluate for inspection or unsupported interactions.`,
    }),
    S({
      ...cq,
      selector: g(mq).annotate({
        description: `Legacy CSS selector that must match an element. Prefer locator.`,
      }),
      locator: g(pq).annotate({
        description: `Playwright selector that must match an element, for example role=button[name='Send'].`,
      }),
      text: g(
        L.annotate({
          description: `Case-sensitive substring that must appear in visible document text.`,
        }),
      ).annotate({
        description: `Case-sensitive substring that must appear in visible document text.`,
      }),
      urlIncludes: g(
        L.annotate({ description: `Substring that must appear in the current absolute URL.` }),
      ).annotate({ description: `Substring that must appear in the current absolute URL.` }),
      timeoutMs: oq,
    })
      .check(
        k((e) =>
          e.selector !== void 0 && e.locator !== void 0
            ? `Provide at most one of selector or locator.`
            : e.selector !== void 0 ||
              e.locator !== void 0 ||
              e.text !== void 0 ||
              e.urlIncludes !== void 0 ||
              `Provide at least one wait condition.`,
        ),
      )
      .annotate({
        description: `Waits until all provided conditions match. Use after click/type when the page changes asynchronously.`,
      }));
  var vq = S({ tag: y, role: D(y), name: y, selector: y, x: b, y: b, width: b, height: b }),
    yq = S({ level: y, text: y, timestamp: y, source: g(y) }),
    bq = S({ url: y, method: y, status: D(b), failed: x, errorText: g(y), timestamp: y }),
    xq = S({
      id: y,
      action: y,
      status: E([`running`, `succeeded`, `failed`, `interrupted`]),
      startedAt: y,
      completedAt: g(y),
      error: g(y),
    });
  (S({
    url: y,
    title: y,
    loading: x,
    visibleText: y,
    interactiveElements: w(vq),
    accessibilityTree: v,
    consoleEntries: w(yq),
    networkEntries: w(bq),
    actionTimeline: w(xq),
    screenshot: S({ mimeType: _(`image/png`), data: y, width: A, height: A }),
  }),
    S({ tabId: AL, recording: x, startedAt: D(y) }),
    S({ id: y, tabId: AL, path: y, mimeType: y, sizeBytes: A, createdAt: y }));
  var Sq = L.check(j(128)),
    Cq = L.check(j(64)),
    wq = S({ clientId: Sq, environmentId: U });
  (S({ ...wq.fields, supportedOperations: g(w(sq)) }),
    S({
      ...wq.fields,
      connectionId: Cq,
      focused: x,
      liveTabs: g(w(S({ threadId: V, tabId: AL, visible: g(x) }))),
    }));
  var Tq = S({
    requestId: L,
    threadId: V,
    tabId: g(AL),
    tabIdExplicit: g(x),
    agentSessionId: g(y),
    operation: sq,
    input: v,
    timeoutMs: A.check(Vg(0)),
  });
  (T([
    S({ type: _(`connected`), connectionId: Cq }),
    S({ type: _(`request`), connectionId: Cq, request: Tq }),
  ]),
    S({
      clientId: Sq,
      connectionId: Cq,
      requestId: L,
      ok: x,
      result: g(v),
      error: g(S({ _tag: L, message: y, detail: g(v) })),
    }));
  var Eq = { environmentId: U, threadId: g(V), providerSessionId: g(L), providerInstanceId: g(q) },
    Dq = class extends F()(`PreviewAutomationUnavailableError`, {
      capability: _(`preview`),
      ...Eq,
    }) {
      get message() {
        return `MCP credential does not grant the ${this.capability} capability: browser preview tools are off for this thread. Do not retry them. To check a page, use a headless browser from the shell, such as Playwright, or curl. The user can turn on "Agent browser access" in Settings; it applies when the agent session next starts.`;
      }
    };
  F()(`McpCapabilityUnavailableError`, { capability: L, ...Eq });
  var Oq = {
      operation: sq,
      environmentId: U,
      threadId: V,
      providerSessionId: L,
      providerInstanceId: q,
    },
    kq = {
      ...Oq,
      clientId: L,
      connectionId: Cq,
      requestId: L,
      tabId: g(AL),
      timeoutMs: A.check(Vg(0)),
    },
    Aq = {
      remoteTag: L,
      remoteMessageLength: A.check(Hg(0)),
      remoteDetailKind: g(E([`null`, `array`, `object`, `string`, `number`, `boolean`])),
      cause: M(),
    },
    jq = {
      remoteTag: g(L),
      remoteMessageLength: g(A.check(Hg(0))),
      remoteDetailKind: g(E([`null`, `array`, `object`, `string`, `number`, `boolean`])),
      cause: g(M()),
    },
    Mq = class extends F()(`PreviewAutomationNoAvailableHostError`, {
      ...Oq,
      clientId: g(L),
      connectionId: g(Cq),
      requestId: g(L),
      tabId: g(AL),
      timeoutMs: g(A.check(Vg(0))),
      ...jq,
    }) {
      get message() {
        return `No preview automation host is available for ${this.operation} in environment ${this.environmentId}. The server-owned browser may be starting or reconnecting. Retry preview_status, then call preview_open if no tab is available. If it remains unavailable, report the browser connection failure.`;
      }
    },
    Nq = class extends F()(`PreviewAutomationUnsupportedClientError`, { ...kq, ...Aq }) {
      get message() {
        return `Preview automation client ${this.clientId} does not support ${this.operation}.`;
      }
    },
    Pq = class extends F()(`PreviewAutomationTabNotFoundError`, { ...kq, ...Aq }) {
      get message() {
        return this.tabId
          ? `Preview tab ${this.tabId} was not found for ${this.operation}. Omit tabId to use the current tab, or call preview_open.`
          : `No active preview tab was found for ${this.operation}. Call preview_open first.`;
      }
    },
    Fq = class extends F()(`PreviewAutomationTimeoutError`, { ...kq, ...jq }) {
      get message() {
        return `Preview automation ${this.operation} timed out after ${this.timeoutMs}ms.`;
      }
    },
    Iq = E([
      `agentMismatch`,
      `humanControl`,
      `tabRequired`,
      `closed`,
      `interrupted`,
      `dialogPending`,
      `tabLimit`,
    ]),
    Lq = class extends F()(`PreviewAutomationControlInterruptedError`, {
      ...kq,
      ...Aq,
      reason: g(Iq),
    }) {
      get message() {
        return this.reason === `agentMismatch`
          ? `This browser tab belongs to another agent session or a human. Open your own tab.`
          : this.reason === `humanControl`
            ? `A human controls this browser tab. Wait until they release control.`
            : this.reason === `tabRequired`
              ? `Multiple browser tabs belong to this session. Pass a tabId from preview_open or from the tabs listed by preview_status.`
              : this.reason === `dialogPending`
                ? `A browser dialog is pending. Read preview_status and resolve it with preview_dialog.`
                : this.reason === `tabLimit`
                  ? `Too many server browser tabs are open. Close one with t3_preview_close, or reuse a tabId from preview_status tabs.`
                  : this.reason === `closed`
                    ? `This browser tab is closed. Call preview_open.`
                    : this.reason === `interrupted`
                      ? `Browser control changed. Take a fresh snapshot before trying again.`
                      : `Preview automation ${this.operation} was interrupted on client ${this.clientId}.`;
      }
    },
    Rq = class extends F()(`PreviewAutomationExecutionError`, { ...kq, ...Aq, reason: g(y) }) {
      get message() {
        return this.reason === void 0
          ? `Preview automation ${this.operation} failed on client ${this.clientId}.`
          : `Preview automation ${this.operation} failed: ${this.reason}`;
      }
    },
    zq = class extends F()(`PreviewAutomationInvalidSelectorError`, {
      ...kq,
      ...Aq,
      selectorKind: g(E([`locator`, `selector`])),
      selectorLength: g(A.check(Hg(0))),
      staleRef: g(x),
    }) {
      get message() {
        return this.staleRef
          ? `This element ref is stale. Navigation, dialogs, and new snapshots replace refs; take a fresh snapshot and use its refs.`
          : this.selectorKind !== void 0 && this.selectorLength !== void 0
            ? `Preview automation ${this.operation} received an invalid ${this.selectorKind} (${this.selectorLength} characters).`
            : `Preview automation ${this.operation} received an invalid selector.`;
      }
    },
    Bq = class extends F()(`PreviewAutomationTargetNotEditableError`, {
      ...kq,
      ...Aq,
      selectorKind: g(E([`focused-element`, `locator`, `selector`])),
      selectorLength: g(A.check(Hg(0))),
    }) {
      get message() {
        return this.selectorKind === `focused-element`
          ? `Preview automation ${this.operation} requires an editable focused element.`
          : this.selectorKind !== void 0 && this.selectorLength !== void 0
            ? `Preview automation ${this.operation} requires an editable ${this.selectorKind} (${this.selectorLength} characters).`
            : `Preview automation ${this.operation} requires an editable target.`;
      }
    },
    Vq = class extends F()(`PreviewAutomationResultTooLargeError`, {
      ...kq,
      ...Aq,
      maximumBytes: g(A.check(Vg(0))),
    }) {
      get message() {
        return this.maximumBytes === void 0
          ? `Preview automation ${this.operation} produced a result that is too large.`
          : `Preview automation ${this.operation} produced a result larger than ${this.maximumBytes} bytes.`;
      }
    },
    Hq = class extends F()(`PreviewAutomationClientDisconnectedError`, kq) {
      get message() {
        return `Preview automation client ${this.clientId} disconnected during ${this.operation}.`;
      }
    },
    Uq = class extends F()(`PreviewAutomationRequestQueueClosedError`, kq) {
      get message() {
        return `Preview automation client ${this.clientId} stopped accepting ${this.operation} requests.`;
      }
    },
    Wq = class extends F()(`PreviewAutomationRemoteUnavailableError`, { ...kq, ...Aq }) {
      get message() {
        return `Preview automation ${this.operation} is unavailable on client ${this.clientId}.`;
      }
    },
    Gq = class extends F()(`PreviewAutomationMalformedResponseError`, kq) {
      get message() {
        return `Preview automation client ${this.clientId} returned a malformed response for ${this.operation}.`;
      }
    };
  (T([
    class extends F()(`PreviewAutomationRecordingTransferError`, { threadId: V, cause: g(M()) }) {
      get message() {
        return `Preview recording could not be saved to the agent environment.`;
      }
    },
    class extends F()(`PreviewAutomationRecordingDesktopUpdateRequiredError`, {
      threadId: V,
      cause: g(M()),
    }) {
      get message() {
        return `Update the desktop app to transfer recordings. The recording remains on the desktop.`;
      }
    },
    class extends F()(`PreviewAutomationRecordingTooLargeError`, { threadId: V, cause: g(M()) }) {
      get message() {
        return `The recording exceeds 50 MiB. Make a shorter recording.`;
      }
    },
    class extends F()(`PreviewAutomationRecordingDeadlineExpiredError`, {
      threadId: V,
      cause: g(M()),
    }) {
      get message() {
        return `The recording transfer deadline expired.`;
      }
    },
    Dq,
    Mq,
    Nq,
    Pq,
    Fq,
    Lq,
    Rq,
    zq,
    Bq,
    Vq,
    Hq,
    Uq,
    Wq,
    Gq,
  ]),
    S({
      requestedUrl: y,
      resolvedUrl: y,
      resolutionKind: E([`direct`, `direct-private-network`]),
      environmentId: U,
    }));
  var Kq = S({
      sampledAt: R,
      cpuUtilization: D(b.check(Gg({ minimum: 0, maximum: 1 }))),
      cpuCount: R,
      availableMemoryBytes: R,
      totalMemoryBytes: R,
    }),
    qq = E([`storage`, `logical`, `all-io`, `unavailable`]),
    Jq = E([
      `server`,
      `server-child`,
      `provider-root`,
      `terminal-root`,
      `electron-main`,
      `electron-renderer`,
      `electron-gpu`,
      `electron-utility`,
      `resource-monitor`,
      `unknown-t3`,
    ]),
    Yq = E([`starting`, `healthy`, `degraded`, `unavailable`, `stopped`]),
    Xq = S({ pid: z, startTimeMs: R }),
    Zq = S({ pid: z, startTimeMs: h(R) }),
    Qq = S({
      cumulativeCpuTime: x,
      currentCpuPercent: x,
      residentMemory: x,
      virtualMemory: x,
      ioBytes: x,
      processStartTime: x,
      processTree: x,
    }),
    $q = S({
      pid: z,
      ppid: R,
      startTimeMs: R,
      runTimeMs: R,
      name: y,
      command: y,
      status: y,
      cpuPercent: b,
      cpuTimeMs: R,
      residentBytes: R,
      virtualBytes: R,
      ioReadBytes: R,
      ioWriteBytes: R,
      ioSemantics: E([`storage`, `all-io`]),
    }),
    eJ = S({
      version: _(3),
      type: _(`configure`),
      rootPid: z,
      sampleIntervalMs: R,
      externalProcesses: w(Zq),
    }),
    tJ = S({ version: _(3), type: _(`setExternalProcesses`), processes: w(Zq) }),
    nJ = S({ version: _(3), type: _(`sampleNow`), requestId: L }),
    rJ = S({ version: _(3), type: _(`processTable`), requestId: L });
  T([
    eJ,
    tJ,
    S({ version: _(3), type: _(`setSampleInterval`), sampleIntervalMs: R }),
    S({ version: _(3), type: _(`setStreaming`), enabled: x }),
    nJ,
    rJ,
    S({ version: _(3), type: _(`readHistory`), requestId: L, windowMs: R }),
    S({ version: _(3), type: _(`shutdown`) }),
  ]);
  var iJ = S({
      version: _(3),
      type: _(`hello`),
      sidecarVersion: L,
      sidecarPid: z,
      platform: L,
      arch: L,
      capabilities: Qq,
    }),
    aJ = S({
      version: _(3),
      type: _(`snapshot`),
      sequence: R,
      sampledAtUnixMs: R,
      collectionDurationMicros: R,
      scannedProcessCount: R,
      retainedProcessCount: R,
      inaccessibleProcessCount: R,
      requestId: h(L),
      externalProcesses: h(w(Zq)),
      processes: w($q),
    }),
    oJ = S({ pid: z, ppid: R, name: y });
  T([
    iJ,
    aJ,
    S({ version: _(3), type: _(`processTable`), requestId: L, processes: w(oJ) }),
    S({ version: _(3), type: _(`historyChunk`), requestId: L, done: x, snapshots: w(aJ) }),
    S({ version: _(3), type: _(`error`), code: L, message: L, recoverable: x }),
  ]);
  var sJ = E([
      `Browser`,
      `Tab`,
      `Utility`,
      `Zygote`,
      `Sandbox helper`,
      `GPU`,
      `Pepper Plugin`,
      `Pepper Plugin Broker`,
      `Unknown`,
    ]),
    cJ = S({
      pid: z,
      creationTimeMs: R,
      type: sJ,
      name: h(y),
      serviceName: h(y),
      cpuPercent: b,
      cumulativeCpuSeconds: h(b),
      idleWakeupsPerSecond: b,
      workingSetBytes: R,
      peakWorkingSetBytes: R,
    }),
    lJ = S({ ...cy.fields, updatedAt: P }),
    uJ = S({
      version: _(1),
      type: _(`desktopTelemetry`),
      sequence: R,
      sampledAtUnixMs: R,
      electronPid: z,
      power: lJ,
      speedLimitPercent: h_(b),
      electronProcesses: w(cJ),
    }),
    dJ = S({ version: _(1), type: _(`desktopTelemetryHello`), electronPid: z }),
    fJ = E([`ready-to-install`, `up-to-date`, `failed`]);
  (T([
    dJ,
    uJ,
    S({
      version: _(1),
      type: _(`desktopUpdateStatus`),
      requestId: h(L),
      outcome: h(fJ),
      reason: h(L),
      state: _B,
    }),
  ]),
    T([
      S({ version: _(1), type: _(`setDiagnosticsDemand`), enabled: x }),
      S({
        version: _(1),
        type: _(`setHostPowerIntervals`),
        activeIntervalMs: z,
        idleIntervalMs: z,
      }),
      S({ version: _(1), type: _(`requestDesktopUpdate`), requestId: L }),
      S({ version: _(1), type: _(`commitDesktopUpdate`), requestId: L }),
      S({ version: _(1), type: _(`cancelDesktopUpdate`), requestId: L }),
    ]));
  var pJ = S({
      identity: Xq,
      ppid: R,
      childPids: w(z),
      depth: R,
      name: y,
      command: y,
      status: y,
      category: Jq,
      electronType: h(sJ),
      electronServiceName: h(y),
      cpuPercent: b,
      cpuTimeMs: R,
      residentBytes: R,
      peakResidentBytes: R,
      virtualBytes: R,
      ioReadBytes: R,
      ioWriteBytes: R,
      ioReadBytesPerSecond: b,
      ioWriteBytesPerSecond: b,
      ioSemantics: qq,
      idleWakeupsPerSecond: h(b),
      runTimeMs: R,
      firstSeenAt: N,
      lastSeenAt: N,
    }),
    mJ = S({
      processCount: R,
      currentCpuPercent: b,
      cpuTimeMs: R,
      currentRssBytes: R,
      peakRssBytes: R,
      ioReadBytes: R,
      ioWriteBytes: R,
      ioReadBytesPerSecond: b,
      ioWriteBytesPerSecond: b,
      processStarts: R,
      processExits: R,
    }),
    hJ = S({ backend: mJ, electron: mJ, monitor: mJ, allT3: mJ }),
    gJ = S({ status: Yq, lastSampleAt: m_(N), lastError: m_(L) }),
    _J = S({
      native: gJ,
      desktop: gJ,
      sidecarVersion: m_(L),
      sidecarPid: m_(z),
      restartCount: R,
      collectionDurationMicros: R,
      scannedProcessCount: R,
      retainedProcessCount: R,
      inaccessibleProcessCount: R,
    }),
    vJ = S({
      readAt: N,
      entries: w(
        S({
          component: L,
          operation: L,
          logicalReadBytes: R,
          logicalWriteBytes: R,
          count: R,
          durationMs: R,
        }),
      ),
    }),
    yJ = S({
      readAt: N,
      sampleIntervalMs: R,
      processes: w(pJ),
      groups: hJ,
      power: cy,
      speedLimitPercent: m_(b),
      attribution: vJ,
      health: _J,
    }),
    bJ = S({ windowMs: R, bucketMs: R }),
    xJ = S({
      startedAt: N,
      endedAt: N,
      avgCpuPercent: b,
      maxCpuPercent: b,
      maxRssBytes: R,
      ioReadBytes: R,
      ioWriteBytes: R,
      maxProcessCount: R,
    }),
    SJ = S({
      identity: Xq,
      ppid: R,
      depth: R,
      name: y,
      command: y,
      category: Jq,
      firstSeenAt: N,
      lastSeenAt: N,
      currentCpuPercent: b,
      avgCpuPercent: b,
      maxCpuPercent: b,
      cpuTimeMs: R,
      currentRssBytes: R,
      peakRssBytes: R,
      ioReadBytes: R,
      ioWriteBytes: R,
      ioSemantics: qq,
      sampleCount: R,
    }),
    CJ = S({
      readAt: N,
      windowMs: R,
      bucketMs: R,
      sampleIntervalMs: R,
      retainedSampleCount: R,
      buckets: w(xJ),
      topProcesses: w(SJ),
      health: _J,
    }),
    wJ = S({ accepted: x, snapshot: yJ });
  (S({
    branch: L.annotate({
      description: `Branch name to create for the worktree (e.g. 'feature/my-change').`,
    }),
    baseRef: g(
      L.annotate({
        description: `Branch or ref the worktree branch starts from. Defaults to the branch currently checked out in the project workspace.`,
      }),
    ),
    startFromOrigin: g(
      x.annotate({
        description: `Fetch origin and start the worktree branch from the remote-tracking commit of baseRef instead of the local ref. Defaults to the server's 'new worktrees start from origin' setting.`,
      }),
    ),
    path: g(
      L.check(Mg(/^(?:[A-Za-z]:[\\/]|[\\/])/)).annotate({
        description: `Absolute filesystem path for the new worktree. Relative paths are rejected. Defaults to the server-managed worktrees directory.`,
      }),
    ),
    runSetupScript: g(
      x.annotate({
        description: `Run the project's configured setup script in the new worktree after handoff. Defaults to true.`,
      }),
    ),
    continuationPrompt: g(
      L.check(j(12e4)).annotate({
        description: `Message queued as the thread's next turn after the handoff. The handoff detaches the current provider session, so pass the remaining work here to automatically resume inside the worktree; omit it to stop after the handoff and wait for the next message.`,
      }),
    ),
  }),
    S({
      worktreePath: L,
      branch: L,
      baseRef: L,
      startedFromOrigin: x,
      setupScript: T([
        S({ status: _(`started`), scriptName: L, terminalId: L }),
        S({ status: _(`no-script`) }),
        S({ status: _(`skipped`) }),
        S({ status: _(`failed`), detail: y }),
      ]),
      continuation: T([
        S({ status: _(`scheduled`), delivery: E([`started`, `queued`, `steered`, `restarted`]) }),
        S({ status: _(`skipped`) }),
        S({ status: _(`failed`), detail: y }),
      ]),
      note: y,
    }),
    S({
      attached: x.annotate({
        description: `True when this thread is already attached to a git worktree.`,
      }),
      worktreePath: D(L),
      branch: D(L),
      projectWorkspaceRoot: L.annotate({
        description: `Root of the project's main workspace checkout.`,
      }),
      defaultStartFromOrigin: x.annotate({
        description: `Server default used by t3_worktree_handoff when startFromOrigin is omitted.`,
      }),
    }),
    F()(`WorktreeMcpFailure`, {
      code: E([
        `capability_denied`,
        `thread_not_found`,
        `project_not_found`,
        `already_in_worktree`,
        `handoff_in_progress`,
        `invalid_request`,
        `operation_failed`,
        `thread_credential_required`,
      ]),
      message: y,
    }));
  var TJ = `~effect/rpc/RpcSchema/StreamSchema`,
    EJ = Lh(Cb);
  function DJ(e, t) {
    return Zh(EJ.ast, { [TJ]: TJ, success: e, error: t });
  }
  var OJ = `~effect/rpc/Rpc`,
    kJ = {
      [OJ]: OJ,
      pipe() {
        return n(this, arguments);
      },
      setSuccess(e) {
        return AJ({
          _tag: this._tag,
          payloadSchema: this.payloadSchema,
          successSchema: e,
          errorSchema: this.errorSchema,
          defectSchema: this.defectSchema,
          annotations: this.annotations,
          middlewares: this.middlewares,
        });
      },
      setError(e) {
        return AJ({
          _tag: this._tag,
          payloadSchema: this.payloadSchema,
          successSchema: this.successSchema,
          errorSchema: e,
          defectSchema: this.defectSchema,
          annotations: this.annotations,
          middlewares: this.middlewares,
        });
      },
      setPayload(e) {
        return AJ({
          _tag: this._tag,
          payloadSchema: Qh(e) ? e : S(e),
          successSchema: this.successSchema,
          errorSchema: this.errorSchema,
          defectSchema: this.defectSchema,
          annotations: this.annotations,
          middlewares: this.middlewares,
        });
      },
      middleware(e) {
        return AJ({
          _tag: this._tag,
          payloadSchema: this.payloadSchema,
          successSchema: this.successSchema,
          errorSchema: this.errorSchema,
          defectSchema: this.defectSchema,
          annotations: this.annotations,
          middlewares: new Set([...this.middlewares, e]),
        });
      },
      prefix(e) {
        return AJ({
          _tag: `${e}${this._tag}`,
          payloadSchema: this.payloadSchema,
          successSchema: this.successSchema,
          errorSchema: this.errorSchema,
          defectSchema: this.defectSchema,
          annotations: this.annotations,
          middlewares: this.middlewares,
        });
      },
      annotate(e, t) {
        return AJ({
          _tag: this._tag,
          payloadSchema: this.payloadSchema,
          successSchema: this.successSchema,
          errorSchema: this.errorSchema,
          defectSchema: this.defectSchema,
          middlewares: this.middlewares,
          annotations: Nr(this.annotations, e, t),
        });
      },
      annotateMerge(e) {
        return AJ({
          _tag: this._tag,
          payloadSchema: this.payloadSchema,
          successSchema: this.successSchema,
          errorSchema: this.errorSchema,
          defectSchema: this.defectSchema,
          middlewares: this.middlewares,
          annotations: Hr(this.annotations, e),
        });
      },
    },
    AJ = (e) => {
      function t() {}
      return (
        Object.setPrototypeOf(t, kJ), Object.assign(t, e), (t.key = `effect/rpc/Rpc/${e._tag}`), t
      );
    },
    Q = (e, t) => {
      let n = t?.success ?? sg,
        r = t?.error ?? rg,
        i = t?.defect ?? M(),
        a;
      return (
        (a = t?.primaryKey
          ? class extends C_(`effect/rpc/Rpc/${e}`)(t.payload) {
              [MI]() {
                return t.primaryKey(this);
              }
            }
          : Qh(t?.payload)
            ? t?.payload
            : t?.payload
              ? S(t?.payload)
              : sg),
        AJ({
          _tag: e,
          payloadSchema: a,
          successSchema: t?.stream ? DJ(n, r) : n,
          errorSchema: t?.stream ? rg : r,
          defectSchema: i,
          annotations: Ar(),
          middlewares: new Set(),
        })
      );
    },
    jJ = `~effect/rpc/Rpc/Wrapper`,
    MJ = (e) => jJ in e,
    NJ = {
      add(...e) {
        let t = new Map(this.requests);
        for (let n of e) t.set(n._tag, n);
        return PJ({ requests: t, annotations: this.annotations });
      },
      merge(...e) {
        let t = new Map(this.requests),
          n = new Map(this.annotations.mapUnsafe);
        for (let r of e) {
          for (let [e, n] of r.requests) t.set(e, n);
          for (let [e, t] of r.annotations.mapUnsafe) n.set(e, t);
        }
        return PJ({ requests: t, annotations: Tr(n) });
      },
      omit(...e) {
        let t = new Map(this.requests);
        for (let n of e) t.delete(n);
        return PJ({ requests: t, annotations: this.annotations });
      },
      middleware(e) {
        let t = new Map();
        for (let [n, r] of this.requests) t.set(n, r.middleware(e));
        return PJ({ requests: t, annotations: this.annotations });
      },
      toHandlers(e) {
        let t = this;
        return zc(function* () {
          let n = yield* Qc(),
            r = Fc(e) ? yield* e : e,
            i = new Map();
          return (
            t.requests.forEach((e, t) => {
              i.set(e.key, { tag: e._tag, handler: r[t], context: n });
            }),
            Tr(i)
          );
        });
      },
      prefix(e) {
        let t = new Map();
        for (let n of this.requests.values()) {
          let r = n.prefix(e);
          t.set(r._tag, r);
        }
        return PJ({ requests: t, annotations: this.annotations });
      },
      toLayer(e) {
        return ic(this.toHandlers(e));
      },
      of: o,
      toLayerHandler(e, t) {
        let n = this;
        return ic(
          zc(function* () {
            let r = yield* Qc(),
              i = Fc(t) ? yield* t : t,
              a = new Map(),
              o = n.requests.get(e);
            return (a.set(o.key, { handler: i, context: r }), Tr(a));
          }),
        );
      },
      accessHandler(e) {
        return $c((t) => {
          let n = this.requests.get(e),
            { handler: r, context: i } = Ir(t, n.key);
          return m((e, t) => {
            t.rpc = n;
            let a = r(e, t),
              o = MJ(a) ? a.value : a;
            return Fc(o) ? el(o, i) : Tb(o, i);
          });
        });
      },
      annotate(e, t) {
        return PJ({ requests: this.requests, annotations: Nr(this.annotations, e, t) });
      },
      annotateRpcs(e, t) {
        return this.annotateRpcsMerge(Mr(e, t));
      },
      annotateMerge(e) {
        return PJ({ requests: this.requests, annotations: Hr(this.annotations, e) });
      },
      annotateRpcsMerge(e) {
        let t = new Map();
        for (let [n, r] of this.requests) t.set(n, r.annotateMerge(Hr(e, r.annotations)));
        return PJ({ requests: t, annotations: this.annotations });
      },
    },
    PJ = (e) =>
      Object.assign(function () {}, NJ, { requests: e.requests, annotations: e.annotations }),
    FJ = (...e) => PJ({ requests: new Map(e.map((e) => [e._tag, e])), annotations: Ar() }),
    IJ = `~effect/rpc/RpcMiddleware`,
    LJ = () => (e, t) => {
      let n = Ct(),
        r;
      n !== 0 && (wt(2), (r = new globalThis.Error()), wt(n));
      function i() {}
      let a = i;
      return (
        Object.setPrototypeOf(i, Object.getPrototypeOf(fr(e))),
        (i.key = e),
        Object.defineProperty(i, "stack", {
          get() {
            return r?.stack;
          },
        }),
        (a[IJ] = IJ),
        (a.error = t?.error ?? rg),
        (a.requiredForClient = t?.requiredForClient ?? !1),
        i
      );
    },
    RJ = 1e3,
    zJ = S({
      id: E([`fetch`, `checkout`, `submodules`, `setup-script`, `agent`]),
      status: E([`pending`, `running`, `done`, `skipped`, `warning`, `failed`]),
      startedAt: D(B),
      endedAt: D(B),
      percent: D(A.check(Gg({ minimum: 0, maximum: 100 }))),
      detail: D(y.check(j(200))),
      tail: w(y.check(j(400))),
    }),
    BJ = S({
      threadId: V,
      phase: E([`running`, `done`, `failed`, `cancelled`]),
      startedAt: B,
      endedAt: D(B),
      branch: D(L),
      baseRef: D(L),
      worktreePath: D(L),
      setupScript: D(S({ name: L, command: L, terminalId: L })),
      stages: w(zJ),
      error: D(y.check(j(RJ))),
      sequence: R,
    }),
    VJ = S({ threadId: V }),
    HJ = D(BJ),
    UJ = S({ threadId: V }),
    WJ = S({ cancelled: x }),
    GJ = S({
      threadId: V,
      turnItemId: tv,
      answer: T([S({ type: _(`save`), secret: L }), S({ type: _(`decline`) })]),
    }),
    KJ = {
      load_failed: `Could not load the secret request.`,
      not_found: `This secret request no longer exists.`,
      already_answered: `This secret request was already answered.`,
      agent_stopped: `The agent that asked has stopped, so this secret can't be used.`,
      store_failed: `Could not store the secret.`,
      record_failed: `Saved the secret, but could not update the request.`,
      invalid_ref: `That secretRef is not valid.`,
      read_failed: `Could not read the secret.`,
      ref_unavailable: `That secretRef was already used or does not exist. Ask the user again with request_secret.`,
      ref_expired: `That secretRef expired. Ask the user again with request_secret.`,
      consume_failed: `Could not use that secretRef. Try again.`,
    },
    qJ = E(Object.keys(KJ)),
    JJ = class extends F()(`SecretRequestError`, { reason: qJ, cause: h(M()) }) {
      get message() {
        return KJ[this.reason];
      }
    },
    $ = {
      projectsList: `projects.list`,
      projectsAdd: `projects.add`,
      projectsRemove: `projects.remove`,
      projectsListEntries: `projects.listEntries`,
      projectsReadFile: `projects.readFile`,
      projectsSearchContents: `projects.searchContents`,
      projectsSearchEntries: `projects.searchEntries`,
      projectsWriteFile: `projects.writeFile`,
      projectsMutate: `projects.mutate`,
      projectsEnsureScratch: `projects.ensureScratch`,
      projectsCreateNew: `projects.createNew`,
      shellOpenInEditor: `shell.openInEditor`,
      filesystemBrowse: `filesystem.browse`,
      filesystemGetMetadata: `filesystem.getMetadata`,
      agentSessionsScan: `agentSessions.scan`,
      agentSessionsImport: `agentSessions.import`,
      assetsCreateUrl: `assets.createUrl`,
      assetsPersistChatAttachments: `assets.persistChatAttachments`,
      attachmentsCreateUploadUrl: `attachments.createUploadUrl`,
      attachmentsDelete: `attachments.delete`,
      mcpAppsCallTool: `mcpApps.callTool`,
      mcpAppsToolInfo: `mcpApps.toolInfo`,
      mcpAppsReadResource: `mcpApps.readResource`,
      mcpAppsUpdateModelContext: `mcpApps.updateModelContext`,
      providerUploadFeedback: `provider.uploadFeedback`,
      providerAuthStart: `provider.auth.start`,
      providerConsumeResetCredit: `provider.consumeResetCredit`,
      providerAuthComplete: `provider.auth.complete`,
      chatGptReconnectProfile: `provider.chatgpt.reconnect-profile`,
      chatGptImportProfile: `provider.chatgpt.import-profile`,
      chatGptHandoffSubscribe: `provider.chatgpt.handoff.subscribe`,
      codexAuthCallbackSubscribe: `provider.codex.auth-callback.subscribe`,
      providerAuthRespond: `provider.auth.respond`,
      providerAuthCancel: `provider.auth.cancel`,
      providerAuthLogout: `provider.auth.logout`,
      providerAuthSubscribe: `provider.auth.subscribe`,
      providerInstallStart: `provider.install.start`,
      providerInstallCancel: `provider.install.cancel`,
      providerInstallSubscribe: `provider.install.subscribe`,
      providerInstallRemove: `provider.install.remove`,
      vcsPull: `vcs.pull`,
      vcsRefreshStatus: `vcs.refreshStatus`,
      vcsListRefs: `vcs.listRefs`,
      vcsCreateWorktree: `vcs.createWorktree`,
      vcsRemoveWorktree: `vcs.removeWorktree`,
      vcsCreateRef: `vcs.createRef`,
      vcsSwitchRef: `vcs.switchRef`,
      vcsInit: `vcs.init`,
      gitRunStackedAction: `git.runStackedAction`,
      gitResolvePullRequest: `git.resolvePullRequest`,
      gitPreparePullRequestThread: `git.preparePullRequestThread`,
      reviewGetDiffPreview: `review.getDiffPreview`,
      reviewGetDiffFileContents: `review.getDiffFileContents`,
      terminalOpen: `terminal.open`,
      terminalAttach: `terminal.attach`,
      terminalObserve: `terminal.observe`,
      terminalWrite: `terminal.write`,
      terminalResize: `terminal.resize`,
      terminalClear: `terminal.clear`,
      terminalRestart: `terminal.restart`,
      terminalClose: `terminal.close`,
      previewOpen: `preview.open`,
      previewNavigate: `preview.navigate`,
      previewResize: `preview.resize`,
      previewAdjust: `preview.adjust`,
      previewRefresh: `preview.refresh`,
      previewClose: `preview.close`,
      previewList: `preview.list`,
      previewClearProfile: `preview.clearProfile`,
      previewReportProfiles: `preview.reportProfiles`,
      previewReportStatus: `preview.reportStatus`,
      deviceConfigure: `device.configure`,
      deviceList: `device.list`,
      deviceTestHost: `device.testHost`,
      deviceOpen: `device.open`,
      deviceClose: `device.close`,
      deviceShutdown: `device.shutdown`,
      deviceDetail: `device.detail`,
      deviceAction: `device.action`,
      serverProbe: `server.probe`,
      serverGetConfig: `server.getConfig`,
      serverRefreshProviders: `server.refreshProviders`,
      serverUpdateProvider: `server.updateProvider`,
      serverUpdateServer: `server.updateServer`,
      serverUpdateServerWithProgress: `server.updateServerWithProgress`,
      serverCommitDesktopUpdate: `server.commitDesktopUpdate`,
      serverUpsertKeybinding: `server.upsertKeybinding`,
      serverRemoveKeybinding: `server.removeKeybinding`,
      serverRunStorageCleanup: `server.runStorageCleanup`,
      serverGetStorageCleanupReport: `server.getStorageCleanupReport`,
      serverGetSettings: `server.getSettings`,
      serverUpdateSettings: `server.updateSettings`,
      serverDiscoverSourceControl: `server.discoverSourceControl`,
      serverSearchAcpRegistry: `server.searchAcpRegistry`,
      serverPrepareAcpRegistryAgent: `server.prepareAcpRegistryAgent`,
      serverUninstallAcpRegistryManagedBinary: `server.uninstallAcpRegistryManagedBinary`,
      serverAcceptAcpRegistryUrlAuth: `server.acceptAcpRegistryUrlAuth`,
      serverListAcpRegistrySessions: `server.listAcpRegistrySessions`,
      serverImportAcpRegistrySession: `server.importAcpRegistrySession`,
      serverDeleteAcpRegistrySession: `server.deleteAcpRegistrySession`,
      serverListAcpRegistryProviders: `server.listAcpRegistryProviders`,
      serverSetAcpRegistryProvider: `server.setAcpRegistryProvider`,
      serverDisableAcpRegistryProvider: `server.disableAcpRegistryProvider`,
      serverLogoutAcpRegistry: `server.logoutAcpRegistry`,
      serverGetTraceDiagnostics: `server.getTraceDiagnostics`,
      serverGetProcessDiagnostics: `server.getProcessDiagnostics`,
      serverGetHostResources: `server.getHostResources`,
      serverGetProcessResourceHistory: `server.getProcessResourceHistory`,
      serverGetResourceTelemetryHistory: `server.getResourceTelemetryHistory`,
      serverRetryResourceTelemetry: `server.retryResourceTelemetry`,
      serverCheckOtlpEndpoint: `server.checkOtlpEndpoint`,
      serverSignalProcess: `server.signalProcess`,
      serverReportClientActivity: `server.reportClientActivity`,
      serverReportHostPowerState: `server.reportHostPowerState`,
      serverGetBackgroundPolicy: `server.getBackgroundPolicy`,
      serverGetUsageSummary: `server.getUsageSummary`,
      serverSearchUsageModels: `server.searchUsageModels`,
      serverRefreshUsageRates: `server.refreshUsageRates`,
      scheduledTasksList: `scheduledTasks.list`,
      scheduledTasksSubscribe: `scheduledTasks.subscribe`,
      scheduledTasksUpsert: `scheduledTasks.upsert`,
      scheduledTasksSetEnabled: `scheduledTasks.setEnabled`,
      scheduledTasksDelete: `scheduledTasks.delete`,
      scheduledTasksRunNow: `scheduledTasks.runNow`,
      scheduledTasksRotateWebhookToken: `scheduledTasks.rotateWebhookToken`,
      secretsAnswerRequest: `secrets.answerRequest`,
      scheduledTasksListWebhookDeliveries: `scheduledTasks.listWebhookDeliveries`,
      scheduledTasksGetWebhookDelivery: `scheduledTasks.getWebhookDelivery`,
      cloudGetRelayClientStatus: `cloud.getRelayClientStatus`,
      cloudInstallRelayClient: `cloud.installRelayClient`,
      pullRequestsList: `pullRequests.list`,
      pullRequestsListStats: `pullRequests.listStats`,
      pullRequestsSummary: `pullRequests.summary`,
      pullRequestsRouting: `pullRequests.routing`,
      pullRequestsRoutingIdentity: `pullRequests.routingIdentity`,
      pullRequestsStack: `pullRequests.stack`,
      pullRequestsLinkedThreads: `pullRequests.linkedThreads`,
      pullRequestsDetail: `pullRequests.detail`,
      pullRequestsPreview: `pullRequests.preview`,
      pullRequestsChecks: `pullRequests.checks`,
      pullRequestsActivity: `pullRequests.activity`,
      pullRequestsReviewGuide: `pullRequests.reviewGuide`,
      pullRequestsGenerateReviewGuide: `pullRequests.generateReviewGuide`,
      pullRequestsThreadComments: `pullRequests.threadComments`,
      pullRequestsDiffFileContents: `pullRequests.diffFileContents`,
      pullRequestsFilesViewed: `pullRequests.filesViewed`,
      pullRequestsSetFilesViewed: `pullRequests.setFilesViewed`,
      pullRequestsRunAction: `pullRequests.runAction`,
      pullRequestsUpdate: `pullRequests.update`,
      pullRequestsComment: `pullRequests.comment`,
      pullRequestsUpdateComment: `pullRequests.updateComment`,
      pullRequestsSubmitReview: `pullRequests.submitReview`,
      pullRequestsReplyToThread: `pullRequests.replyToThread`,
      pullRequestsSetThreadResolution: `pullRequests.setThreadResolution`,
      pullRequestsSetReaction: `pullRequests.setReaction`,
      pullRequestsInvalidate: `pullRequests.invalidate`,
      pullRequestsReportState: `pullRequests.reportState`,
      pullRequestsSubscribeRefreshes: `pullRequests.subscribeRefreshes`,
      pullRequestsReviewerCandidates: `pullRequests.reviewerCandidates`,
      pullRequestsRequestReviewers: `pullRequests.requestReviewers`,
      pullRequestsLabelCandidates: `pullRequests.labelCandidates`,
      pullRequestsSetLabels: `pullRequests.setLabels`,
      sourceControlLookupRepository: `sourceControl.lookupRepository`,
      sourceControlCloneRepository: `sourceControl.cloneRepository`,
      sourceControlPublishRepository: `sourceControl.publishRepository`,
      projectCloneStart: `projectClone.start`,
      projectCloneCancel: `projectClone.cancel`,
      projectCloneRetry: `projectClone.retry`,
      subscribeProjectClones: `subscribeProjectClones`,
      subscribeVcsStatus: `subscribeVcsStatus`,
      subscribeWorktreeSetup: `subscribeWorktreeSetup`,
      worktreeSetupCancel: `worktreeSetup.cancel`,
      subscribeTerminalEvents: `subscribeTerminalEvents`,
      subscribeTerminalMetadata: `subscribeTerminalMetadata`,
      subscribePreviewEvents: `subscribePreviewEvents`,
      subscribeDiscoveredLocalServers: `subscribeDiscoveredLocalServers`,
      subscribeDeviceState: `subscribeDeviceState`,
      subscribeServerConfig: `subscribeServerConfig`,
      subscribeServerLifecycle: `subscribeServerLifecycle`,
      subscribeAuthAccess: `subscribeAuthAccess`,
      subscribeBackgroundPolicy: `subscribeBackgroundPolicy`,
      subscribeResourceTelemetry: `subscribeResourceTelemetry`,
    },
    YJ = Q($.serverUpsertKeybinding, { payload: zH, success: VH, error: T([EL, J]) }),
    XJ = Q($.serverRemoveKeybinding, { payload: BH, success: HH, error: T([EL, J]) }),
    ZJ = Q($.serverProbe, { payload: S({}), success: S({}), error: J }),
    QJ = Q($.serverGetConfig, { payload: S({}), success: LH, error: T([EL, aB, J]) }),
    $J = Q($.serverRefreshProviders, {
      payload: S({ instanceId: g(q), cwd: g(L), fresh: g(x), refreshModels: g(x) }),
      success: iU,
      error: T([J, jV]),
    }),
    eY = Q($.serverUpdateProvider, { payload: aU, success: iU, error: T([oU, J]) }),
    tY = T([jV, J]),
    nY = Q($.providerConsumeResetCredit, { payload: eO, success: nO, error: T([jV, tO, J]) }),
    rY = Q($.providerAuthStart, { payload: CV, success: TV, error: tY }),
    iY = Q($.providerAuthRespond, { payload: wV, success: TV, error: tY }),
    aY = Q($.providerAuthComplete, { payload: EV, success: TV, error: tY }),
    oY = Q($.chatGptReconnectProfile, { payload: PV, success: D(MV), error: tY }),
    sY = Q($.chatGptImportProfile, { payload: FV, success: TV, error: tY }),
    cY = Q($.chatGptHandoffSubscribe, { payload: IV, success: LV, error: tY, stream: !0 }),
    lY = Q($.codexAuthCallbackSubscribe, { payload: _V, success: vV, error: tY, stream: !0 }),
    uY = Q($.providerAuthCancel, { payload: DV, success: TV, error: tY }),
    dY = Q($.providerAuthLogout, { payload: gV, success: TV, error: tY }),
    fY = Q($.providerAuthSubscribe, { payload: gV, success: TV, error: tY, stream: !0 }),
    pY = Q($.providerInstallStart, { payload: gV, success: kV, error: tY }),
    mY = Q($.providerInstallCancel, { payload: AV, success: kV, error: tY }),
    hY = Q($.providerInstallSubscribe, { payload: gV, success: kV, error: tY, stream: !0 }),
    gY = Q($.providerInstallRemove, { payload: gV, success: kV, error: tY }),
    _Y = Q($.serverUpdateServer, { payload: sU, success: cU, error: T([fU, J]) }),
    vY = Q($.serverUpdateServerWithProgress, {
      payload: sU,
      success: dU,
      error: T([fU, J]),
      stream: !0,
    }),
    yY = Q($.serverCommitDesktopUpdate, { payload: lU, success: cU, error: T([fU, J]) }),
    bY = Q($.serverRunStorageCleanup, { payload: S({}), success: eB, error: T([aB, J]) }),
    xY = Q($.serverGetStorageCleanupReport, {
      payload: S({}),
      success: D(eB),
      stream: !0,
      error: J,
    }),
    SY = Q($.serverGetSettings, { payload: S({}), success: nB, error: T([aB, J]) }),
    CY = Q($.serverUpdateSettings, {
      payload: S({ patch: sB, providerInstanceMutation: h(ay) }),
      success: nB,
      error: T([aB, J]),
    }),
    wY = Q($.serverDiscoverSourceControl, { payload: S({}), success: Xw, error: J }),
    TY = Q($.serverSearchAcpRegistry, { payload: ky, success: jy, error: T([ub, J]) }),
    EY = Q($.serverPrepareAcpRegistryAgent, { payload: My, success: Ny, error: T([ub, J]) }),
    DY = Q($.serverUninstallAcpRegistryManagedBinary, {
      payload: Py,
      success: Fy,
      error: T([ub, J]),
    }),
    OY = Q($.serverAcceptAcpRegistryUrlAuth, { payload: By, success: Vy, error: J }),
    kY = Q($.serverListAcpRegistrySessions, { payload: Gy, success: Ky, error: T([ub, J]) }),
    AY = Q($.serverImportAcpRegistrySession, { payload: qy, success: Jy, error: T([ub, J]) }),
    jY = Q($.serverDeleteAcpRegistrySession, { payload: Yy, success: Xy, error: T([ub, J]) }),
    MY = Q($.serverListAcpRegistryProviders, { payload: eb, success: tb, error: T([ub, J]) }),
    NY = Q($.serverSetAcpRegistryProvider, { payload: nb, success: rb, error: T([ub, J]) }),
    PY = Q($.serverDisableAcpRegistryProvider, { payload: ib, success: ab, error: T([ub, J]) }),
    FY = Q($.serverLogoutAcpRegistry, { payload: ob, success: sb, error: T([ub, J]) }),
    IY = Q($.serverGetTraceDiagnostics, { payload: S({}), success: xH, error: J }),
    LY = Q($.serverGetProcessDiagnostics, { payload: S({}), success: CH, error: J }),
    RY = Q($.serverGetHostResources, { payload: S({}), success: Kq, error: J }),
    zY = Q($.serverGetProcessResourceHistory, { payload: wH, success: OH, error: J }),
    BY = Q($.serverGetResourceTelemetryHistory, { payload: bJ, success: CJ, error: J }),
    VY = Q($.serverRetryResourceTelemetry, { payload: S({}), success: wJ, error: J }),
    HY = Q($.serverCheckOtlpEndpoint, { payload: pH, success: mH, error: J }),
    UY = Q($.serverGetUsageSummary, { payload: ER, success: AR, error: T([J, jR]) }),
    WY = Q($.serverSearchUsageModels, { payload: OR, success: kR, error: T([J, jR]) }),
    GY = Q($.serverRefreshUsageRates, { payload: S({}), success: TR, error: J }),
    KY = Q($.serverSignalProcess, { payload: kH, success: AH, error: J }),
    qY = Q($.cloudGetRelayClientStatus, { payload: S({}), success: gI, error: J }),
    JY = Q($.cloudInstallRelayClient, {
      payload: S({}),
      success: vI,
      error: T([bI, J]),
      stream: !0,
    }),
    YY = Q($.serverReportClientActivity, { payload: fy, error: J }),
    XY = Q($.serverReportHostPowerState, { payload: cy, error: J }),
    ZY = Q($.serverGetBackgroundPolicy, { payload: S({}), success: py, error: J }),
    QY = T([kE, AE, J]),
    $Y = Q($.pullRequestsList, { payload: IT, success: zT, error: QY }),
    eX = Q($.pullRequestsListStats, { payload: YT, success: XT, error: QY }),
    tX = Q($.pullRequestsRouting, { payload: BT, success: UT, error: QY }),
    nX = Q($.pullRequestsRoutingIdentity, { payload: VT, success: HT, error: QY }),
    rX = Q($.pullRequestsSummary, { payload: BT, success: KT, error: QY }),
    iX = Q($.pullRequestsStack, { payload: BT, success: D(qT), error: QY }),
    aX = Q($.pullRequestsLinkedThreads, { payload: BT, success: WT, error: QY }),
    oX = Q($.pullRequestsDetail, { payload: BT, success: $T, error: QY }),
    sX = Q($.pullRequestsPreview, { payload: BT, success: GT, error: QY }),
    cX = Q($.pullRequestsChecks, { payload: BT, success: D(eE), error: QY }),
    lX = Q($.pullRequestsActivity, { payload: BT, success: tE, error: QY }),
    uX = Q($.pullRequestsReviewGuide, { payload: BT, success: TW, error: QY }),
    dX = Q($.pullRequestsGenerateReviewGuide, { payload: BT, success: TW, error: QY }),
    fX = Q($.pullRequestsThreadComments, { payload: bE, success: xE, error: QY }),
    pX = Q($.pullRequestsDiffFileContents, { payload: aE, success: oE, error: QY }),
    mX = Q($.pullRequestsFilesViewed, { payload: BT, success: cE, error: QY }),
    hX = Q($.pullRequestsSetFilesViewed, { payload: uE, success: sg, error: QY }),
    gX = Q($.pullRequestsRunAction, { payload: fE, success: sg, error: QY }),
    _X = Q($.pullRequestsUpdate, { payload: hE, success: sg, error: QY }),
    vX = Q($.pullRequestsComment, { payload: mE, success: sg, error: QY }),
    yX = Q($.pullRequestsUpdateComment, { payload: gE, success: sg, error: QY }),
    bX = Q($.pullRequestsSubmitReview, { payload: yE, success: sg, error: QY }),
    xX = Q($.pullRequestsReplyToThread, { payload: SE, success: sg, error: QY }),
    SX = Q($.pullRequestsSetThreadResolution, { payload: CE, success: sg, error: QY }),
    CX = Q($.pullRequestsSetReaction, { payload: wE, success: sg, error: QY }),
    wX = Q($.pullRequestsInvalidate, { payload: ZT, success: sg, error: QY }),
    TX = Q($.pullRequestsReportState, { payload: QT, success: sg, error: QY }),
    EX = Q($.pullRequestsSubscribeRefreshes, { payload: S({}), success: R, error: J, stream: !0 }),
    DX = Q($.pullRequestsReviewerCandidates, { payload: BT, success: CT, error: QY }),
    OX = Q($.pullRequestsRequestReviewers, { payload: TE, success: sg, error: QY }),
    kX = Q($.pullRequestsLabelCandidates, { payload: BT, success: wT, error: QY }),
    AX = Q($.pullRequestsSetLabels, { payload: EE, success: sg, error: QY }),
    jX = Q($.sourceControlLookupRepository, { payload: zw, success: Rw, error: T([Qw, J]) }),
    MX = Q($.sourceControlCloneRepository, { payload: Bw, success: Vw, error: T([Qw, J]) }),
    NX = Q($.projectCloneStart, { payload: gW, success: _W, error: T([Qw, EW, J]) }),
    PX = Q($.projectCloneCancel, { payload: vW, success: yW, error: J }),
    FX = Q($.projectCloneRetry, { payload: vW, success: yW, error: T([Qw, J]) }),
    IX = Q($.subscribeProjectClones, { payload: mW, success: hW, error: J, stream: !0 }),
    LX = Q($.sourceControlPublishRepository, { payload: Hw, success: Ww, error: T([Qw, J]) }),
    RX = Q($.projectsSearchEntries, { payload: SD, success: wD, error: T([jD, J]) }),
    zX = Q($.projectsSearchContents, { payload: TD, success: ED, error: T([MD, J]) }),
    BX = Q($.projectsListEntries, { payload: DD, success: OD, error: T([ND, J]) }),
    VX = Q($.projectsReadFile, { payload: PD, success: FD, error: T([RD, J]) }),
    HX = Q($.projectsWriteFile, { payload: zD, success: BD, error: T([WD, J]) }),
    UX = Q($.projectsMutate, { payload: yD, success: hD, error: T([bD, J]) }),
    WX = Q($.projectsEnsureScratch, { payload: S({}), success: VD, error: T([EW, J]) }),
    GX = Q($.projectsCreateNew, { payload: HD, success: UD, error: T([EW, J]) }),
    KX = Q($.shellOpenInEditor, { payload: VV, error: T([YV, J]) }),
    qX = Q($.filesystemGetMetadata, { payload: QG, success: $G, error: J }),
    JX = Q($.filesystemBrowse, { payload: eK, success: tK, error: T([iK, J]) }),
    YX = Q($.agentSessionsScan, { payload: oK, success: cK, error: T([pK, J]) }),
    XX = Q($.agentSessionsImport, { payload: lK, success: fK, error: T([dK, uK, pK, J]) }),
    ZX = Q($.assetsCreateUrl, { payload: gK, success: vK, error: T([IK, J]) }),
    QX = Q($.assetsPersistChatAttachments, { payload: cw, success: lw, error: T([uw, J]) }),
    $X = Q($.attachmentsCreateUploadUrl, { payload: yK, success: bK, error: T([SK, J]) }),
    eZ = Q($.attachmentsDelete, { payload: xK, error: J }),
    tZ = Q($.mcpAppsCallTool, { payload: KK, success: qK, error: T([eq, J]) }),
    nZ = Q($.mcpAppsToolInfo, { payload: JK, success: YK, error: T([eq, J]) }),
    rZ = Q($.mcpAppsUpdateModelContext, { payload: QK, error: T([eq, J]) }),
    iZ = Q($.mcpAppsReadResource, { payload: XK, success: ZK, error: T([eq, J]) }),
    aZ = Q($.providerUploadFeedback, { payload: pV, success: mV, error: T([hV, J]) }),
    oZ = Q($.subscribeVcsStatus, { payload: MU, success: XU, error: T([sW, J]), stream: !0 }),
    sZ = Q($.vcsPull, { payload: NU, success: rW, error: T([aW, J]) }),
    cZ = Q($.vcsRefreshStatus, { payload: jU, success: YU, error: T([sW, J]) }),
    lZ = Q($.subscribeWorktreeSetup, { payload: VJ, success: HJ, error: J, stream: !0 }),
    uZ = Q($.worktreeSetupCancel, { payload: UJ, success: WJ, error: J }),
    dZ = Q($.gitRunStackedAction, { payload: PU, success: lW, error: T([sW, J]), stream: !0 }),
    fZ = Q($.gitResolvePullRequest, { payload: LU, success: $U, error: T([sW, J]) }),
    pZ = Q($.gitPreparePullRequestThread, { payload: RU, success: eW, error: T([sW, J]) }),
    mZ = Q($.vcsListRefs, { payload: FU, success: ZU, error: T([aW, J]) }),
    hZ = Q($.vcsCreateWorktree, { payload: IU, success: QU, error: T([aW, J]) }),
    gZ = Q($.vcsRemoveWorktree, { payload: zU, error: T([aW, J]) }),
    _Z = Q($.vcsCreateRef, { payload: BU, success: VU, error: T([aW, J]) }),
    vZ = Q($.vcsSwitchRef, { payload: HU, success: tW, error: T([aW, J]) }),
    yZ = Q($.vcsInit, { payload: UU, error: T([Mw, J]) }),
    bZ = Q($.reviewGetDiffPreview, { payload: LK, success: UK, error: T([WK, J]) }),
    xZ = Q($.reviewGetDiffFileContents, { payload: VK, success: HK, error: T([WK, J]) }),
    SZ = Q($.terminalOpen, { payload: HB, success: ZB, error: T([dV, J]) }),
    CZ = Q($.terminalAttach, { payload: UB, success: uV, error: T([dV, J]), stream: !0 }),
    wZ = Q($.terminalObserve, { payload: WB, success: uV, error: T([dV, J]), stream: !0 }),
    TZ = Q($.terminalWrite, { payload: GB, error: T([dV, J]) }),
    EZ = Q($.terminalResize, { payload: KB, error: T([dV, J]) }),
    DZ = Q($.terminalClear, { payload: qB, error: T([dV, J]) }),
    OZ = Q($.terminalRestart, { payload: JB, success: ZB, error: T([dV, J]) }),
    kZ = Q($.terminalClose, { payload: YB, error: T([dV, J]) }),
    AZ = Q($.previewOpen, { payload: XL, success: YL, error: T([mR, J]) }),
    jZ = Q($.previewNavigate, { payload: ZL, success: YL, error: T([mR, J]) }),
    MZ = Q($.previewResize, { payload: eR, success: YL, error: T([mR, J]) }),
    NZ = Q($.previewAdjust, { payload: tR, success: YL, error: T([mR, J]) }),
    PZ = Q($.previewRefresh, { payload: $L, error: T([mR, J]) }),
    FZ = Q($.previewClose, { payload: nR, error: T([mR, J]) }),
    IZ = Q($.previewList, { payload: aR, success: oR, error: J }),
    LZ = Q($.previewClearProfile, { payload: iR, error: T([pR, J]) }),
    RZ = Q($.previewReportProfiles, { payload: rR, error: J }),
    zZ = Q($.previewReportStatus, { payload: QL, error: T([mR, J]) }),
    BZ = Q($.subscribePreviewEvents, { payload: S({}), success: cR, error: J, stream: !0 }),
    VZ = Q($.subscribeDiscoveredLocalServers, {
      payload: S({ configuredUrls: g(OL) }),
      success: lR,
      error: J,
      stream: !0,
    }),
    HZ = Q($.deviceTestHost, { payload: FI, success: HI, error: T([fL, J]) }),
    UZ = Q($.deviceList, { payload: KI, success: GI, error: T([fL, J]) }),
    WZ = Q($.deviceConfigure, { payload: qI, success: GI, error: T([fL, J]) }),
    GZ = Q($.deviceOpen, { payload: JI, success: WI, error: T([fL, J]) }),
    KZ = Q($.deviceClose, { payload: YI, error: T([fL, J]) }),
    qZ = Q($.deviceShutdown, { payload: XI, error: T([fL, J]) }),
    JZ = Q($.deviceDetail, { payload: aL, success: tL, error: T([fL, J]) }),
    YZ = Q($.deviceAction, { payload: iL, success: tL, error: T([fL, J]) }),
    XZ = Q($.subscribeDeviceState, { payload: S({}), success: GI, error: J, stream: !0 }),
    ZZ = Q(AM.dispatchCommand, {
      payload: sN.dispatchCommand.input,
      success: sN.dispatchCommand.output,
      error: T([GM, J]),
    }),
    QZ = Q(AM.getTurnDiff, {
      payload: sN.getTurnDiff.input,
      success: sN.getTurnDiff.output,
      error: T([_w, J]),
    }),
    $Z = Q(AM.getFullThreadDiff, {
      payload: sN.getFullThreadDiff.input,
      success: sN.getFullThreadDiff.output,
      error: T([vw, J]),
    }),
    eQ = Q(AM.searchThread, { payload: rN, success: aN, error: T([oN, J]) }),
    tQ = Q(AM.searchThreadStream, { payload: rN, success: aN, error: T([oN, J]), stream: !0 }),
    nQ = Q(AM.searchThreads, { payload: UG, success: WG, error: T([GG, J]) }),
    rQ = Q(AM.getArchivedShellSnapshot, {
      payload: sN.getArchivedShellSnapshot.input,
      success: sN.getArchivedShellSnapshot.output,
      error: T([qM, J]),
    }),
    iQ = Q(AM.getThreadProjection, {
      payload: sN.getThreadProjection.input,
      success: sN.getThreadProjection.output,
      error: T([KM, J]),
    }),
    aQ = Q(AM.stopWorkflow, {
      payload: sN.stopWorkflow.input,
      success: sN.stopWorkflow.output,
      error: T([XM, J]),
    }),
    oQ = Q(AM.getWorkflowScript, {
      payload: sN.getWorkflowScript.input,
      success: sN.getWorkflowScript.output,
      error: T([nN, J]),
    }),
    sQ = Q(AM.getTurnItem, {
      payload: sN.getTurnItem.input,
      success: sN.getTurnItem.output,
      error: T([KM, J]),
    }),
    cQ = Q(AM.launchThread, {
      payload: sN.launchThread.input,
      success: sN.launchThread.output,
      error: T([JM, J]),
    }),
    lQ = Q(AM.subscribeArchivedShell, {
      payload: sN.subscribeArchivedShell.input,
      success: sN.subscribeArchivedShell.output,
      error: T([qM, J]),
      stream: !0,
    }),
    uQ = Q(AM.subscribeShell, {
      payload: sN.subscribeShell.input,
      success: sN.subscribeShell.output,
      error: T([qM, J]),
      stream: !0,
    }),
    dQ = Q(AM.subscribeThread, {
      payload: sN.subscribeThread.input,
      success: sN.subscribeThread.output,
      error: T([KM, J]),
      stream: !0,
    }),
    fQ = Q($.subscribeTerminalEvents, { payload: S({}), success: lV, error: J, stream: !0 }),
    pQ = Q($.subscribeTerminalMetadata, { payload: S({}), success: $B, error: J, stream: !0 }),
    mQ = Q($.subscribeServerConfig, {
      payload: S({ environmentThemes: g(x), usageLimitSources: g(x), usageLimitsCommand: g(x) }),
      success: $H,
      error: T([EL, aB, J]),
      stream: !0,
    }),
    hQ = Q($.subscribeServerLifecycle, { payload: S({}), success: rU, error: J, stream: !0 }),
    gQ = Q($.scheduledTasksList, { payload: JW, success: YW, error: T([fG, J]) }),
    _Q = Q($.scheduledTasksSubscribe, { payload: JW, success: YW, error: T([fG, J]), stream: !0 }),
    vQ = Q($.scheduledTasksUpsert, { payload: XW, success: lG, error: T([fG, J]) }),
    yQ = Q($.scheduledTasksSetEnabled, { payload: ZW, success: lG, error: T([fG, J]) }),
    bQ = Q($.scheduledTasksDelete, { payload: QW, success: uG, error: T([fG, J]) }),
    xQ = Q($.scheduledTasksRunNow, { payload: $W, success: dG, error: T([fG, J]) }),
    SQ = Q($.scheduledTasksRotateWebhookToken, { payload: eG, success: lG, error: T([fG, J]) }),
    CQ = Q($.secretsAnswerRequest, { payload: GJ, error: T([JJ, J]) }),
    wQ = Q($.scheduledTasksListWebhookDeliveries, { payload: aG, success: oG, error: T([fG, J]) }),
    TQ = Q($.scheduledTasksGetWebhookDelivery, { payload: sG, success: cG, error: T([fG, J]) }),
    EQ = Q($.subscribeAuthAccess, { payload: S({}), success: Fx, error: T([Px, J]), stream: !0 }),
    DQ = Q($.subscribeBackgroundPolicy, { payload: S({}), success: py, error: J, stream: !0 }),
    OQ = Q($.subscribeResourceTelemetry, { payload: S({}), success: yJ, error: J, stream: !0 }),
    kQ = class extends LJ()(`t3/contracts/RpcScopeAuthorization`, { error: J }) {};
  (FJ(
    ZJ,
    QJ,
    $J,
    eY,
    nY,
    rY,
    aY,
    oY,
    sY,
    cY,
    lY,
    iY,
    uY,
    dY,
    fY,
    pY,
    mY,
    hY,
    gY,
    _Y,
    vY,
    yY,
    YJ,
    XJ,
    bY,
    xY,
    SY,
    CY,
    wY,
    TY,
    EY,
    DY,
    OY,
    kY,
    AY,
    jY,
    MY,
    NY,
    PY,
    FY,
    IY,
    LY,
    RY,
    zY,
    BY,
    VY,
    HY,
    UY,
    WY,
    GY,
    KY,
    gQ,
    _Q,
    vQ,
    yQ,
    bQ,
    xQ,
    SQ,
    CQ,
    wQ,
    TQ,
    YY,
    XY,
    ZY,
    qY,
    JY,
    $Y,
    eX,
    rX,
    tX,
    nX,
    iX,
    aX,
    oX,
    sX,
    cX,
    lX,
    uX,
    dX,
    fX,
    pX,
    mX,
    hX,
    gX,
    _X,
    vX,
    yX,
    bX,
    xX,
    SX,
    CX,
    wX,
    TX,
    EX,
    DX,
    OX,
    kX,
    AX,
    jX,
    MX,
    LX,
    NX,
    PX,
    FX,
    IX,
    BX,
    VX,
    zX,
    RX,
    WX,
    GX,
    HX,
    UX,
    KX,
    JX,
    qX,
    YX,
    XX,
    ZX,
    QX,
    $X,
    eZ,
    tZ,
    nZ,
    iZ,
    rZ,
    aZ,
    oZ,
    lZ,
    uZ,
    sZ,
    cZ,
    dZ,
    fZ,
    pZ,
    mZ,
    hZ,
    gZ,
    _Z,
    vZ,
    yZ,
    bZ,
    xZ,
    SZ,
    CZ,
    wZ,
    TZ,
    EZ,
    DZ,
    OZ,
    kZ,
    fQ,
    pQ,
    AZ,
    jZ,
    MZ,
    NZ,
    PZ,
    FZ,
    IZ,
    LZ,
    RZ,
    zZ,
    BZ,
    VZ,
    WZ,
    UZ,
    HZ,
    GZ,
    KZ,
    qZ,
    JZ,
    YZ,
    XZ,
    mQ,
    hQ,
    EQ,
    DQ,
    OQ,
    ZZ,
    oQ,
    aQ,
    sQ,
    QZ,
    $Z,
    nQ,
    eQ,
    tQ,
    rQ,
    iQ,
    cQ,
    lQ,
    uQ,
    dQ,
  ).middleware(kQ),
    $.serverRunStorageCleanup,
    $.serverCheckOtlpEndpoint,
    $.pullRequestsRunAction,
    $.pullRequestsUpdate,
    $.pullRequestsComment,
    $.pullRequestsUpdateComment,
    $.pullRequestsSubmitReview,
    $.pullRequestsReplyToThread,
    $.pullRequestsSetThreadResolution,
    $.pullRequestsSetReaction,
    $.pullRequestsSetFilesViewed,
    $.pullRequestsRequestReviewers,
    $.pullRequestsSetLabels,
    $.pullRequestsGenerateReviewGuide,
    $.sourceControlCloneRepository,
    $.sourceControlPublishRepository,
    $.projectCloneStart,
    $.projectCloneCancel,
    $.projectCloneRetry,
    $.vcsPull,
    $.gitRunStackedAction,
    $.gitPreparePullRequestThread,
    $.vcsCreateWorktree,
    $.vcsRemoveWorktree,
    $.vcsCreateRef,
    $.vcsSwitchRef,
    $.vcsInit,
    $.scheduledTasksUpsert,
    $.scheduledTasksSetEnabled,
    $.scheduledTasksDelete,
    $.scheduledTasksRunNow,
    $.scheduledTasksRotateWebhookToken,
    AM.stopWorkflow,
    Yh(RU));
  var AQ = (e) =>
      e === null ||
      (typeof e == `object` &&
        `type` in e &&
        typeof e.type == `string` &&
        `message` in e &&
        typeof e.message == `string` &&
        `defaultValue` in e &&
        typeof e.defaultValue == `string`),
    jQ = (e, n) =>
      t(
        `${e.access.httpBase}/upload?${new URLSearchParams({ threadId: e.threadId, tabId: e.tabId, chooser: n }).toString()}`,
        e.access,
      );
  async function MQ(e, t) {
    let n = new FormData();
    for (let e of t) n.append(`file`, e, e.name ?? `file`);
    let r = await fetch(e.uploadUrl, {
      method: `POST`,
      body: n,
      credentials: e.credentials ? `include` : `omit`,
    });
    if (!r.ok) throw Error((await r.text()) || `The upload was refused.`);
  }
  var NQ = (e, n) =>
      t(
        `${e.access.httpBase}/download?${new URLSearchParams({ threadId: e.threadId, tabId: e.tabId, id: n }).toString()}`,
        e.access,
      ),
    PQ = (e) => +!!e.altKey | (e.ctrlKey ? 2 : 0) | (e.metaKey ? 4 : 0) | (e.shiftKey ? 8 : 0),
    FQ = JSON.stringify({ type: `ack` });
  function IQ(e, n) {
    let r = new URLSearchParams({
      threadId: e.threadId,
      tabId: e.tabId,
      maxWidth: String(Math.max(1, Math.round(e.maxWidth))),
      maxHeight: String(Math.max(1, Math.round(e.maxHeight))),
    });
    e.interactive === !1 && r.set(`interactive`, `false`);
    let i = t(`${e.access.wsBase}/ws?${r.toString()}`, e.access),
      a = !1,
      o = null,
      s = null,
      c = 0,
      l = null,
      u = null,
      d = () => {
        if (a) return;
        let t = new WebSocket(i);
        ((t.binaryType = `arraybuffer`), (o = t));
        let r = !1;
        (t.addEventListener(`open`, () => {
          o === t && ((r = !0), n.onConnectedChange(!0));
        }),
          t.addEventListener(`message`, (r) => {
            if (o !== t) return;
            if (r.data instanceof ArrayBuffer) {
              ((c = 0), t.send(FQ), n.onFrame(r.data));
              return;
            }
            if (typeof r.data != `string`) return;
            let i;
            try {
              i = JSON.parse(r.data);
            } catch {
              return;
            }
            if (typeof i != `object` || !i) return;
            let {
              type: a,
              x: s,
              y: d,
              width: f,
              height: ee,
              editable: te,
              canOperate: ne,
              controller: re,
              generation: ie,
              dialog: ae,
              text: oe,
              id: se,
              fileName: ce,
              sizeBytes: le,
              multiple: ue,
              accept: de,
              phase: fe,
              sequence: pe,
              tabId: me,
            } = i;
            if (
              a === `fileChooser` &&
              typeof se == `string` &&
              typeof ue == `boolean` &&
              typeof de == `string`
            )
              ((u = se),
                n.onFileChooser?.({
                  multiple: ue,
                  accept: de,
                  uploadUrl: jQ(e, se),
                  credentials: e.access.credentials,
                }));
            else if (a === `fileChooserClosed` && typeof se == `string`) {
              if (u !== se) return;
              ((u = null), n.onFileChooser?.(null));
            } else if (
              a === `pointer` &&
              (fe === `move` || fe === `click`) &&
              typeof s == `number` &&
              typeof d == `number` &&
              typeof pe == `number`
            )
              n.onPointer?.({ phase: fe, x: s, y: d, sequence: pe });
            else if (a === `popup` && typeof me == `string`) n.onPopup?.(me);
            else if (a === `clipboard` && typeof oe == `string`) n.onClipboard?.(oe);
            else if (
              a === `download` &&
              typeof se == `string` &&
              typeof ce == `string` &&
              typeof le == `number`
            )
              n.onDownload?.({ fileName: ce, sizeBytes: le, url: NQ(e, se) });
            else if (a === `viewport` && typeof f == `number` && typeof ee == `number`)
              ((c = 0), n.onViewport({ width: f, height: ee }));
            else if (
              a === `probe` &&
              typeof s == `number` &&
              typeof d == `number` &&
              typeof te == `boolean`
            )
              n.onProbe?.({ x: s, y: d, editable: te });
            else if (
              a === `control` &&
              typeof ne == `boolean` &&
              (re === `agent` || re === `you` || re === `another-viewer` || re === `unclaimed`) &&
              typeof ie == `number` &&
              AQ(ae)
            ) {
              let e = { canOperate: ne, controller: re, generation: ie, dialog: ae };
              ((l = e), n.onControl?.(e));
            }
          }),
          t.addEventListener(`close`, (e) => {
            if (o === t && ((o = null), (l = null), r && n.onConnectedChange(!1), !a)) {
              if (e.code === 4404) {
                ((a = !0), n.onGone?.());
                return;
              }
              if (e.code === 4503) {
                ((a = !0), n.onHostSetup?.(RQ(e.reason)));
                return;
              }
              if (e.code === 1008 || e.code === 4401 || (!r && e.code === 1006)) {
                ((a = !0), n.onUnauthorized());
                return;
              }
              s = setTimeout(d, Math.min(500 * 2 ** c++, 1e4));
            }
          }),
          t.addEventListener(`error`, () => t.close()));
      };
    return (
      d(),
      {
        send: (e) =>
          o?.readyState !== WebSocket.OPEN ||
          !l?.canOperate ||
          (e.type !== `takeControl` && l.controller !== `you`)
            ? !1
            : (o.send(JSON.stringify(e)), !0),
        stop: () => {
          ((a = !0), s !== null && clearTimeout(s));
          let e = o;
          ((o = null), (l = null), e?.close());
        },
      }
    );
  }
  function LQ(e, t) {
    let n = e.getContext(`2d`),
      r = !1,
      i = !1,
      a = null,
      o = (s) => {
        ((i = !0),
          createImageBitmap(new Blob([s], { type: `image/jpeg` }))
            .then((i) => {
              if (r || !n) {
                i.close();
                return;
              }
              ((e.width !== i.width || e.height !== i.height) &&
                ((e.width = i.width), (e.height = i.height)),
                n.drawImage(i, 0, 0),
                i.close(),
                t?.());
            })
            .catch(() => void 0)
            .finally(() => {
              i = !1;
              let e = a;
              ((a = null), e && !r && o(e));
            }));
      };
    return {
      paint: (e) => {
        r || (i ? (a = e) : o(e));
      },
      stop: () => {
        ((r = !0), (a = null));
      },
    };
  }
  var RQ = (e) => {
      try {
        let t = JSON.parse(e);
        if ((t.need === `sandbox` || t.need === `libraries`) && typeof t.command == `string`)
          return { need: t.need, command: t.command };
      } catch {}
      return { need: `sandbox`, command: `sudo npx t3 browser setup` };
    },
    zQ = (e) => (e === 0 ? `left` : e === 1 ? `middle` : e === 2 ? `right` : `none`),
    BQ = 150,
    VQ = 8,
    HQ = 500,
    UQ = 4,
    WQ = 16,
    GQ = 2,
    KQ = 24,
    qQ = 1e4,
    JQ = `​`,
    YQ = 700,
    XQ = `<svg width="20" height="20" viewBox="0 0 24 24" fill="#fff" stroke="#3b82f6" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4.037 4.688a.495.495 0 0 1 .651-.651l16 6.5a.5.5 0 0 1-.063.947l-6.124 1.58a2 2 0 0 0-1.438 1.435l-1.579 6.126a.5.5 0 0 1-.947.063z"/></svg>`,
    ZQ = null;
  function QQ() {
    (ZQ?.stop(), (ZQ = null));
  }
  function $Q(e) {
    ZQ?.command(e);
  }
  function e$() {
    ZQ?.togglePictureInPicture();
  }
  function t$(e) {
    QQ();
    let t = (e) => {
        window.ReactNativeWebView.postMessage(JSON.stringify(e));
      },
      { interactive: n } = e;
    document.body.style.background = e.background;
    let r = document.createElement(`div`),
      i = document.createElement(`canvas`);
    (i.setAttribute(`role`, `img`), i.setAttribute(`aria-label`, `Browser page`));
    let a = document.createElement(`div`);
    ((a.className = `agent-cursor`),
      a.setAttribute(`aria-hidden`, `true`),
      (a.innerHTML = XQ),
      r.append(i, a));
    let o = document.createElement(`textarea`);
    (o.setAttribute(`aria-label`, `Browser page input`),
      (o.autocapitalize = `off`),
      (o.autocomplete = `off`),
      (o.spellcheck = !1),
      o.setAttribute(`autocorrect`, `off`),
      (o.disabled = !0),
      document.body.replaceChildren(r, ...(n ? [o] : [])));
    let s = !1,
      c = !1,
      l = null,
      u = null,
      d = null,
      f = null,
      ee = null,
      te = null,
      ne = null,
      re = null,
      ie = null,
      ae = null,
      oe = 0,
      se = { left: 1, middle: 1, right: 1, none: 1 },
      ce = null,
      le = null,
      ue = !1,
      de = null,
      fe = null,
      pe = (e) => {
        ((c = e === `streaming`), t({ type: `status`, status: e }));
      },
      me = LQ(i, () => {
        (!c && !s && pe(`streaming`), Ge());
      }),
      he = (e) => f?.send(e) ?? !1,
      ge = () => {
        (ne !== null && cancelAnimationFrame(ne),
          ie !== null && cancelAnimationFrame(ie),
          (ne = ie = null),
          (re = ae = null),
          (oe = 0),
          Object.assign(se, { left: 1, middle: 1, right: 1, none: 1 }),
          (ce = le = de = fe = null),
          (ue = !1),
          (o.value = JQ),
          (o.disabled = !0),
          o.blur());
      },
      _e = () => {
        if ((f && pe(`connecting`), f?.stop(), (ee = null), ge(), !d || s)) return;
        let r = IQ(
          {
            access: e.access,
            threadId: e.threadId,
            tabId: e.tabId,
            maxWidth: d.width,
            maxHeight: d.height,
            interactive: n,
          },
          {
            onFrame: (e) => me.paint(e),
            onPointer: Ce,
            onClipboard: (e) => t({ type: `clipboard`, text: e }),
            onDownload: (e) => t({ type: `download`, ...e }),
            onFileChooser: (e) => t({ type: `fileChooser`, chooser: e }),
            onViewport: (e) => {
              (l?.width !== e.width || l.height !== e.height) &&
                ((l = e), t({ type: `viewport`, width: e.width, height: e.height }));
            },
            onProbe: (e) => {
              if (ee?.controller !== `you`) return;
              let t = ce;
              if (t && t.x === e.x && t.y === e.y) {
                if (
                  ((le = {
                    clientX: t.clientX,
                    clientY: t.clientY,
                    editable: e.editable,
                    time: performance.now(),
                  }),
                  t.end === `touching`)
                ) {
                  t.editable = e.editable;
                  return;
                }
                ((ce = null),
                  t.end !== `panned` &&
                    (t.end !== `acted` || t.editable !== e.editable) &&
                    (e.editable ? o.focus({ preventScroll: !0 }) : o.blur()));
              }
            },
            onControl: (e) => {
              let i = ee;
              ((ee = e),
                t({ type: `control`, ...e }),
                e.controller === `you` && (a.style.opacity = `0`),
                e.controller === `you`
                  ? ((o.disabled = !n),
                    n && u && i?.controller !== `you` && r.send({ type: `resize`, ...u }))
                  : ge());
            },
            onConnectedChange: (e) => {
              if (!e) {
                ((ee = null), ge(), c && pe(`connecting`));
                return;
              }
            },
            onUnauthorized: () => t({ type: `unauthorized` }),
            onGone: () => t({ type: `gone` }),
            onHostSetup: (e) => t({ type: `hostSetup`, ...e }),
          },
        );
        f = r;
      },
      ve = () => {
        te = null;
        let e = r.getBoundingClientRect();
        if (e.width < 1 || e.height < 1) return;
        let t = { width: Math.round(e.width), height: Math.round(e.height) };
        if (u?.width === t.width && u.height === t.height) return;
        ((u = t), n && he({ type: `resize`, ...t }));
        let i = Math.min(window.devicePixelRatio || 1, GQ),
          a = Math.max(t.width, t.height),
          o = Math.round((n ? t.width : a) * i),
          s = Math.round((n ? t.height : a) * i);
        (d && d.width >= o && d.height >= s) ||
          ((d = { width: Math.max(o, d?.width ?? 0), height: Math.max(s, d?.height ?? 0) }), _e());
      },
      ye = new ResizeObserver(() => {
        (te !== null && clearTimeout(te), (te = setTimeout(ve, u === null ? 0 : BQ)));
      });
    ye.observe(r);
    let be = window.visualViewport,
      xe = () => {
        be && ((r.style.top = `${be.offsetTop}px`), (r.style.height = `${be.height}px`));
      };
    n && (be?.addEventListener(`resize`, xe), be?.addEventListener(`scroll`, xe));
    let Se = null,
      Ce = (e) => {
        if (!l || i.width === 0 || i.height === 0 || ee?.controller === `you`) return;
        let t = { width: i.clientWidth, height: i.clientHeight },
          n = Math.min(t.width / i.width, t.height / i.height),
          r = i.width * n,
          o = i.height * n,
          s = (t.width - r) / 2 + (e.x * r) / l.width,
          c = (t.height - o) / 2 + (e.y * o) / l.height;
        if (
          ((a.style.transform = `translate3d(${s}px, ${c}px, 0)`),
          (a.style.opacity = `1`),
          e.phase === `click`)
        ) {
          a.querySelector(`.ping`)?.remove();
          let e = document.createElement(`span`);
          ((e.className = `ping`), a.prepend(e));
        }
        (Se !== null && clearTimeout(Se),
          (Se = setTimeout(() => {
            a.style.opacity = ee?.controller === `agent` ? `0.35` : `0`;
          }, YQ)));
      },
      we = (e, t, n) => {
        if (!l || i.width === 0 || i.height === 0 || ee?.controller !== `you`) return null;
        let r = i.getBoundingClientRect(),
          a = Math.min(r.width / i.width, r.height / i.height),
          o = i.width * a,
          s = i.height * a;
        if (!(o > 0 && s > 0)) return null;
        let c = l.width / o,
          u = (e - r.left - (r.width - o) / 2) * c,
          d = (t - r.top - (r.height - s) / 2) * (l.height / s);
        return !(u >= 0 && d >= 0 && u <= l.width && d <= l.height) && !n
          ? null
          : {
              x: Math.min(Math.max(u, 0), l.width),
              y: Math.min(Math.max(d, 0), l.height),
              scale: c,
            };
      },
      Te = () => {
        (ne !== null && cancelAnimationFrame(ne), (ne = null));
        let e = re;
        ((re = null), e && he(e));
      },
      Ee = (e, t, n) => {
        ((re = {
          type: `wheel`,
          x: e.x,
          y: e.y,
          deltaX: (re?.deltaX ?? 0) + t,
          deltaY: (re?.deltaY ?? 0) + n,
          modifiers: 0,
        }),
          (ne ??= requestAnimationFrame(Te)));
      },
      De = () => {
        (ie !== null && cancelAnimationFrame(ie), (ie = null), ae && he(ae), (ae = null));
      },
      Oe = (e, t) => {
        let n = fe,
          r =
            n &&
            n.button === t &&
            e.timeStamp - n.time < HQ &&
            Math.hypot(e.clientX - n.x, e.clientY - n.y) < UQ
              ? n.count + 1
              : 1;
        return ((fe = { time: e.timeStamp, x: e.clientX, y: e.clientY, count: r, button: t }), r);
      },
      ke = (e, t, n, r, i = e.buttons, a = se[r]) => ({
        type: `mouse`,
        action: n,
        x: t.x,
        y: t.y,
        button: r,
        buttons: i,
        clickCount: a,
        modifiers: PQ(e),
      }),
      Ae = (e) => {
        if (ee?.controller !== `you` || !e.isPrimary) return;
        if ((e.preventDefault(), e.pointerType !== `touch`)) {
          let t = we(e.clientX, e.clientY, !1);
          if (!t) return;
          (i.setPointerCapture(e.pointerId),
            o.focus({ preventScroll: !0 }),
            De(),
            Te(),
            (oe = e.buttons & 7));
          let n = zQ(e.button);
          ((se[n] = Oe(e, n)), he(ke(e, t, `down`, n)));
          return;
        }
        de = { start: e, last: e, panning: !1 };
        let t = we(e.clientX, e.clientY, !1);
        if (!t) {
          ce = null;
          return;
        }
        let n = le,
          r =
            n !== null &&
            performance.now() - n.time < qQ &&
            Math.hypot(e.clientX - n.clientX, e.clientY - n.clientY) <= KQ;
        ((ce = {
          x: t.x,
          y: t.y,
          clientX: e.clientX,
          clientY: e.clientY,
          editable: r ? n.editable : null,
          end: `touching`,
        }),
          he({ type: `probe`, x: t.x, y: t.y }));
      },
      je = (e) => {
        if (ee?.controller !== `you`) return;
        if (e.pointerType !== `touch`) {
          let t = we(e.clientX, e.clientY, oe !== 0);
          if (!t) return;
          let n = oe ^ (e.buttons & 7);
          if (oe !== 0 && n !== 0) {
            (De(), Te());
            for (let r of [1, 2, 4]) {
              if (!(n & r)) continue;
              let i = r === 1 ? `left` : r === 2 ? `right` : `middle`,
                a = (e.buttons & r) !== 0;
              ((oe ^= r), a && (se[i] = Oe(e, i)), he(ke(e, t, a ? `down` : `up`, i, oe)));
            }
          }
          let r = oe & 1 ? `left` : oe & 2 ? `right` : oe & 4 ? `middle` : `none`;
          ((ae = ke(e, t, `move`, r, e.buttons, 0)), (ie ??= requestAnimationFrame(De)));
          return;
        }
        if (
          !de ||
          de.start.pointerId !== e.pointerId ||
          (!de.panning &&
            Math.hypot(e.clientX - de.start.clientX, e.clientY - de.start.clientY) < VQ)
        )
          return;
        de.panning = !0;
        let t = we(e.clientX, e.clientY, !0);
        (t &&
          Ee(t, (de.last.clientX - e.clientX) * t.scale, (de.last.clientY - e.clientY) * t.scale),
          (de.last = e));
      },
      Me = (e, t) => {
        let n = oe;
        if (n === 0) return;
        ((oe = t ? 0 : e.buttons & 7), De(), Te());
        let r = we(e.clientX, e.clientY, !0);
        if (r)
          for (let t of [1, 2, 4])
            n & t &&
              (oe & t) === 0 &&
              ((n &= ~t), he(ke(e, r, `up`, t === 1 ? `left` : t === 2 ? `right` : `middle`, n)));
      },
      Ne = (e) => {
        let t = e.type === `pointercancel`;
        if (e.pointerType !== `touch`) {
          Me(e, t);
          return;
        }
        let n = de;
        if (!n || n.start.pointerId !== e.pointerId) return;
        de = null;
        let r = ce;
        if (t || n.panning) {
          r && (r.end = `panned`);
          return;
        }
        let i = we(e.clientX, e.clientY, !1);
        if (!i) return;
        (r?.editable === !0 ? o.focus({ preventScroll: !0 }) : r?.editable === !1 && o.blur(),
          r && (r.end = r.editable === null ? `late` : `acted`),
          Te());
        let a = Oe(e, `left`),
          s = { x: i.x, y: i.y, modifiers: 0 };
        (he({ type: `mouse`, action: `move`, ...s, button: `none`, buttons: 0, clickCount: 0 }),
          he({ type: `mouse`, action: `down`, ...s, button: `left`, buttons: 1, clickCount: a }),
          he({ type: `mouse`, action: `up`, ...s, button: `left`, buttons: 0, clickCount: a }));
      },
      Pe = (e) => {
        let t = we(e.clientX, e.clientY, !1);
        if (!t) return;
        e.preventDefault();
        let n =
          e.deltaMode === WheelEvent.DOM_DELTA_LINE
            ? WQ
            : e.deltaMode === WheelEvent.DOM_DELTA_PAGE
              ? (l?.height ?? 0)
              : 1;
        Ee(t, e.deltaX * n, e.deltaY * n);
      },
      Fe = (e) => e.preventDefault(),
      Ie = (e, t) => {
        if (
          ee?.controller !== `you` ||
          t.isComposing ||
          t.keyCode === 229 ||
          t.key === `Process` ||
          t.key === `Unidentified`
        )
          return;
        let n = t.ctrlKey || t.metaKey;
        if ((n && t.key.toLowerCase() === `v`) || (t.shiftKey && t.key === `Insert`)) return;
        let r = n ? void 0 : [...t.key].length === 1 ? t.key : t.key === `Enter` ? `\r` : void 0;
        (he({
          type: `key`,
          action: e,
          key: t.key,
          code: t.code,
          keyCode: t.keyCode,
          ...(e === `down` && r !== void 0 ? { text: r } : {}),
          modifiers: PQ(t),
        }),
          (ue =
            e === `down` && !n && (r !== void 0 || t.key === `Backspace` || t.key === `Delete`)),
          n || t.preventDefault());
      },
      Le = () => {
        ((o.value = JQ), o.setSelectionRange(1, 1));
      },
      Re = (e, t) => {
        for (let n of [`down`, `up`])
          he({ type: `key`, action: n, key: e, code: e, keyCode: t, modifiers: 0 });
      };
    n &&
      (i.addEventListener(`pointerdown`, Ae),
      i.addEventListener(`pointermove`, je),
      i.addEventListener(`pointerup`, Ne),
      i.addEventListener(`pointercancel`, Ne),
      i.addEventListener(`wheel`, Pe, { passive: !1 }),
      i.addEventListener(`mousedown`, Fe),
      i.addEventListener(`contextmenu`, Fe),
      o.addEventListener(`keydown`, (e) => Ie(`down`, e)),
      o.addEventListener(`keyup`, (e) => Ie(`up`, e)),
      o.addEventListener(`input`, (e) => {
        if (e instanceof InputEvent && e.isComposing) return;
        let t = e instanceof InputEvent ? e.inputType : ``;
        if (ue) ue = !1;
        else if (t === `deleteContentBackward`) Re(`Backspace`, 8);
        else if (t === `deleteContentForward`) Re(`Delete`, 46);
        else {
          let e = o.value,
            t = e.startsWith(JQ) ? e.slice(1) : e;
          t && he({ type: `text`, text: t });
        }
        Le();
      }),
      o.addEventListener(`compositionend`, (e) => {
        let t = e.data.startsWith(JQ) ? e.data.slice(1) : e.data;
        (t && he({ type: `text`, text: t }), Le());
      }),
      o.addEventListener(`focus`, Le),
      o.addEventListener(`copy`, Fe),
      o.addEventListener(`cut`, Fe),
      Le());
    let ze = HTMLVideoElement.prototype,
      Be =
        n &&
        typeof i.captureStream == `function` &&
        typeof CanvasCaptureMediaStreamTrack == `function` &&
        ((document.pictureInPictureEnabled === !0 &&
          typeof ze.requestPictureInPicture == `function`) ||
          typeof ze.webkitSetPresentationMode == `function`),
      Ve = null,
      He = null,
      Ue = () =>
        Ve !== null &&
        (document.pictureInPictureElement === Ve ||
          Ve.webkitPresentationMode === `picture-in-picture`),
      We = (e) =>
        t({ type: `pictureInPicture`, supported: Be, active: Ue(), ...(e ? { detail: e } : {}) }),
      Ge = () => {
        Ve && (Ve.readyState === HTMLMediaElement.HAVE_NOTHING || Ue()) && He?.requestFrame();
      };
    if (Be) {
      let t = i.captureStream(0),
        [n] = t.getVideoTracks(),
        a = document.createElement(`video`);
      ((a.muted = !0), (a.playsInline = !0), (a.srcObject = t));
      for (let e of [
        `enterpictureinpicture`,
        `leavepictureinpicture`,
        `webkitpresentationmodechanged`,
      ])
        a.addEventListener(e, () => {
          (Ue() || a.pause(), We());
        });
      (r.prepend(a),
        (i.style.background = e.background),
        (Ve = a),
        (He = n instanceof CanvasCaptureMediaStreamTrack ? n : null));
    }
    ((ZQ = {
      stop: () => {
        ((s = !0),
          ye.disconnect(),
          be?.removeEventListener(`resize`, xe),
          be?.removeEventListener(`scroll`, xe),
          te !== null && clearTimeout(te),
          ne !== null && cancelAnimationFrame(ne),
          ie !== null && cancelAnimationFrame(ie),
          Se !== null && clearTimeout(Se),
          me.stop(),
          f?.stop(),
          (f = null),
          He?.stop(),
          Ve && (Ve.srcObject = null));
      },
      command: (e) => {
        he(e);
      },
      togglePictureInPicture: () => {
        let e = Ve;
        if (!e) return;
        if (Ue()) {
          document.pictureInPictureElement
            ? document.exitPictureInPicture().catch(() => {})
            : e.webkitSetPresentationMode?.(`inline`);
          return;
        }
        if (e.readyState === HTMLMediaElement.HAVE_NOTHING) {
          We(`The page has not shown yet. Try again in a moment.`);
          return;
        }
        (He?.requestFrame(), i.getContext(`2d`)?.drawImage(i, 0, 0));
        let t = (t) => {
          (e.pause(), We(t instanceof Error ? t.message : `Picture in picture is unavailable.`));
        };
        if (document.pictureInPictureEnabled) {
          let n = e.requestPictureInPicture();
          Promise.all([n, e.play()]).catch(t);
        } else (e.webkitSetPresentationMode?.(`picture-in-picture`), e.play().catch(t));
      },
    }),
      window.addEventListener(`pagehide`, QQ, { once: !0 }),
      pe(`connecting`),
      n && We());
  }
  function n$(e) {
    let t = document.createElement(`input`);
    ((t.type = `file`),
      (t.accept = e.accept),
      (t.multiple = e.multiple),
      (t.style.display = `none`));
    let n = async (n) => {
      try {
        await MQ(e, n);
      } catch (e) {
        window.ReactNativeWebView.postMessage(
          JSON.stringify({ type: `uploadError`, detail: String(e) }),
        );
      } finally {
        t.remove();
      }
    };
    ((t.onchange = () => void n(Array.from(t.files ?? []))),
      (t.oncancel = () => void n([])),
      document.body.append(t),
      t.click());
  }
  return (
    (e.command = $Q),
    (e.pictureInPicture = e$),
    (e.selectFiles = n$),
    (e.start = t$),
    (e.stop = QQ),
    e
  );
})({});
