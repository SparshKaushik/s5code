var T3McpAppHost = (function (e) {
  Object.defineProperty(e, Symbol.toStringTag, { value: `Module` });
  var t = (e, t) => {
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
    n = function (e, t) {
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
    r = (e) => e;
  function i(e) {
    return typeof e == `function`;
  }
  function a(e) {
    return typeof e == `object` && !!e && !Array.isArray(e);
  }
  function o(e) {
    return (typeof e == `object` && !!e) || i(e);
  }
  var s = n(2, (e, t) => o(e) && t in e),
    ee = `2026-01-26`,
    c = (e) => {
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
    l = new WeakSet(),
    u = (e) => new Uint8Array(e.buffer, e.byteOffset, e.byteLength),
    d = 0,
    f = () => {
      d++;
    },
    p = `~effect/Hash`,
    m = (e) => {
      switch (typeof e) {
        case `number`:
          return b(e);
        case `bigint`:
          return x(e.toString(10));
        case `string`:
          return x(e);
        case `function`:
        case `object`:
          if (e === null) break;
          if (e instanceof Date)
            return Number.isNaN(e.getTime()) ? x(`Invalid Date`) : x(e.toISOString());
          if (e instanceof RegExp) return x(e.toString());
          {
            if (l.has(e)) return h(e);
            let t = w.get(e);
            if (t !== void 0) return t;
            if (T.has(e)) return (f(), x(`[Circular]`));
            T.add(e);
            let n = d,
              r;
            try {
              r =
                `~effect/Hash` in e
                  ? e[p]()
                  : typeof e == `function`
                    ? h(e)
                    : e instanceof DataView
                      ? S(u(e))
                      : Array.isArray(e) || ArrayBuffer.isView(e)
                        ? S(e)
                        : e instanceof Map
                          ? ie(e)
                          : e instanceof Set
                            ? oe(e)
                            : ne(e);
            } finally {
              T.delete(e);
            }
            return (n === d && w.set(e, r), r);
          }
      }
      return v(g(x(String(e))));
    },
    h = (e) => (C.has(e) || C.set(e, v((Math.random() * 4294967296) | 0)), C.get(e)),
    g = (e) => (
      (e ^= e >>> 16),
      (e = Math.imul(e, 2246822507)),
      (e ^= e >>> 13),
      (e = Math.imul(e, 3266489909)),
      e ^ (e >>> 16)
    ),
    _ = n(2, (e, t) => g(Math.imul(e, 2654435761) + Math.imul(t, 2246822507))),
    v = (e) => (e & 3221225471) | ((e >>> 1) & 1073741824),
    y = new DataView(new ArrayBuffer(8)),
    b = (e) => {
      let t = e | 0;
      return t === e
        ? v(t)
        : (y.setFloat64(0, e === e ? e : NaN), v(_(y.getInt32(0), y.getInt32(4))));
    },
    x = (e) => {
      let t = 5381,
        n = e.length;
      for (; n;) t = (t * 33) ^ e.charCodeAt(--n);
      return v(t);
    },
    te = (e, t) => {
      let n = 12289;
      for (let r of t) n ^= _(m(r), m(e[r]));
      return v(n);
    },
    ne = (e) => te(e, c(e)),
    re = (e, t) => (n) => {
      let r = e;
      for (let e of n) r ^= t(e);
      return v(r);
    },
    S = (e) => {
      let t = 6151;
      for (let n of e) t = _(t, m(n));
      return v(t);
    },
    ie = re(x(`Map`), ([e, t]) => _(m(e), m(t))),
    ae = x(`Set`),
    oe = re(ae, (e) => _(ae, m(e))),
    C = new WeakMap(),
    w = new WeakMap(),
    T = new WeakSet(),
    E = `~effect/Equal`;
  function D() {
    return arguments.length === 1 ? (e) => O(e, arguments[0]) : O(arguments[0], arguments[1]);
  }
  function O(e, t) {
    if (e === t) return !0;
    if (e == null || t == null) return !1;
    let n = typeof e;
    return n === typeof t
      ? n === `number` && e !== e && t !== t
        ? !0
        : (n !== `object` && n !== `function`) || l.has(e) || l.has(t)
          ? !1
          : se(e, t)
      : !1;
  }
  function se(e, t) {
    let n = k.length;
    for (let r = n; r-- > 0;) if (k[r] === e && A[r] === t) return !0;
    if (n) return ce(e, t);
    let r = le.get(e);
    r || le.set(e, (r = new WeakMap()));
    let i = r.get(t);
    return (i === void 0 && r.set(t, (i = ce(e, t))), i);
  }
  function ce(e, t) {
    (k.push(e), A.push(t));
    try {
      return ue(e, t);
    } finally {
      (k.pop(), A.pop());
    }
  }
  var k = [],
    A = [],
    le = new WeakMap();
  function ue(e, t) {
    if (m(e) !== m(t)) return !1;
    if (e instanceof Date) {
      if (!(t instanceof Date)) return !1;
      let n = e.getTime(),
        r = t.getTime();
      return n === r || (Number.isNaN(n) && Number.isNaN(r));
    }
    if (e instanceof RegExp) return t instanceof RegExp && e.toString() === t.toString();
    let n = _e(e);
    if (n !== _e(t) || (typeof e == `function` && !n)) return !1;
    if (n) return e[E](t);
    if (Array.isArray(e)) return !Array.isArray(t) || e.length !== t.length ? !1 : de(e, t);
    if (ArrayBuffer.isView(e)) {
      let n = e instanceof DataView;
      return !ArrayBuffer.isView(t) || e.byteLength !== t.byteLength || n !== t instanceof DataView
        ? !1
        : n
          ? fe(u(e), u(t))
          : fe(e, t);
    }
    return e instanceof Map
      ? !(t instanceof Map) || e.size !== t.size
        ? !1
        : me(e, t, he, ge)
      : e instanceof Set
        ? !(t instanceof Set) || e.size !== t.size
          ? !1
          : me(e, t, m, O)
        : pe(e, t);
  }
  function de(e, t) {
    for (let n = 0; n < e.length; n++) if (!O(e[n], t[n])) return !1;
    return !0;
  }
  function fe(e, t) {
    if (e.length !== t.length) return !1;
    for (let n = 0; n < e.length; n++) if (e[n] !== t[n]) return !1;
    return !0;
  }
  function pe(e, t) {
    let n = c(e),
      r = c(t);
    if (n.size !== r.size) return !1;
    for (let i of n) if (!r.has(i) || !O(e[i], t[i])) return !1;
    return !0;
  }
  function me(e, t, n, r) {
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
  var he = (e) => m(e[0]),
    ge = (e, t) => O(e[0], t[0]) && O(e[1], t[1]),
    _e = (e) => s(e, E),
    j = Symbol.for(`~effect/Redactable`),
    ve = (e) => s(e, j);
  function ye(e) {
    return ve(e) ? be(e) : e;
  }
  function be(e) {
    return e[j](globalThis[`~effect/Fiber/currentFiber`]?.context ?? xe);
  }
  var M = new Map(),
    xe = {
      "~effect/Context": {},
      base: M,
      depth: 0,
      mapUnsafe: M,
      pipe() {
        return t(this, arguments);
      },
    };
  function N(e, t) {
    let n = t?.space ?? 0,
      r = new WeakSet(),
      i = n ? (typeof n == `number` ? ` `.repeat(n) : n) : ``,
      a = (e) => i.repeat(e),
      o = (e, t) => {
        let n = e?.constructor;
        return n && n !== Object.prototype.constructor && n.name ? `${n.name}(${t})` : t;
      },
      ee = (e) => {
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
        if (r.has(e)) return Se;
        r.add(e);
        let l;
        if (j in e) l = c(be(e), n);
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
        else if (e instanceof Date) l = we(e);
        else if (
          !t?.ignoreToString &&
          s(e, `toString`) &&
          typeof e.toString == `function` &&
          e.toString !== Object.prototype.toString &&
          e.toString !== Array.prototype.toString
        ) {
          let t = Te(e);
          l = e instanceof Error && e.cause !== void 0 ? `${t} (cause: ${c(e.cause, n)})` : t;
        } else if (Symbol.iterator in e) l = `${e.constructor.name}(${c(Array.from(e), n)})`;
        else {
          let t = ee(e);
          if (!i || t.length <= 1) {
            let r = `{${t.map((t) => `${Ce(t)}:${c(Ee(e, t), n)}`).join(`,`)}}`;
            l = o(e, r);
          } else {
            let r = `{\n${t.map((t) => `${a(n + 1)}${Ce(t)}: ${c(Ee(e, t), n + 1)}`).join(`,
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
  var Se = `[Circular]`;
  function Ce(e) {
    return typeof e == `string` ? JSON.stringify(e) : String(e);
  }
  function we(e) {
    try {
      return e.toISOString();
    } catch {
      return `Invalid Date`;
    }
  }
  function Te(e) {
    try {
      let t = e.toString();
      return typeof t == `string` ? t : String(t);
    } catch {
      return `[toString threw]`;
    }
  }
  function Ee(e, t) {
    try {
      return e[t];
    } catch {
      return `[property access threw]`;
    }
  }
  var P = Symbol.for(`nodejs.util.inspect.custom`),
    F = (e) => {
      try {
        return (
          (e = ye(e)),
          s(e, `toJSON`) && i(e.toJSON) && e.toJSON.length === 0
            ? e.toJSON()
            : Array.isArray(e)
              ? e.map(F)
              : e
        );
      } catch {
        return `[toJSON threw]`;
      }
    },
    De = class {
      constructor(e) {
        ((this.value = e), (this.done = !1));
      }
      next(e) {
        return this.done
          ? ((this.value = e), this)
          : ((this.done = !0), { value: this.value, done: !1 });
      }
    };
  function Oe(e, t, n) {
    t === `__proto__`
      ? Object.defineProperty(e, t, { value: n, writable: !0, enumerable: !0, configurable: !0 })
      : (e[t] = n);
  }
  function ke(e, t) {
    for (let n of Reflect.ownKeys(t))
      Object.prototype.propertyIsEnumerable.call(t, n) && Oe(e, n, t[n]);
  }
  var I = `~effect/Effect`,
    L = `~effect/Exit`,
    Ae = { _A: r, _E: r, _R: r },
    R = `${I}/identifier`,
    z = `${I}/args`,
    B = `${I}/evaluate`,
    je = `${I}/successCont`,
    V = `${I}/failureCont`,
    Me = `${I}/ensureCont`,
    Ne = {
      pipe() {
        return t(this, arguments);
      },
      toJSON() {
        return { ...this };
      },
      toString() {
        return N(this.toJSON(), { ignoreToString: !0, space: 2 });
      },
      [P]() {
        return this.toJSON();
      },
    },
    Pe = {
      [I]: Ae,
      ...Ne,
      [Symbol.iterator]() {
        return new De(this);
      },
      toJSON() {
        return { _id: `Effect`, op: this[R], ...(z in this ? { args: this[z] } : void 0) };
      },
    },
    Fe = (e) => s(e, L),
    H = `~effect/Cause`,
    U = `~effect/Cause/Reason`,
    Ie = (e) => s(e, H),
    W = class {
      constructor(e) {
        ((this[H] = H), (this.reasons = e));
      }
      pipe() {
        return t(this, arguments);
      }
      toJSON() {
        return { _id: `Cause`, failures: this.reasons.map((e) => e.toJSON()) };
      }
      toString() {
        return `Cause(${N(this.reasons)})`;
      }
      [P]() {
        return this.toJSON();
      }
      [E](e) {
        return (
          Ie(e) &&
          this.reasons.length === e.reasons.length &&
          this.reasons.every((t, n) => D(t, e.reasons[n]))
        );
      }
      [p]() {
        return S(this.reasons);
      }
    },
    Le = new WeakMap(),
    Re = class {
      [U];
      annotations;
      _tag;
      constructor(e, t, n) {
        if (((this[U] = U), (this._tag = e), t !== G && typeof n == `object` && n && t.size > 0)) {
          let e = Le.get(n);
          (e && (t = new Map([...e, ...t])), Le.set(n, t));
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
        return t(this, arguments);
      }
      toString() {
        return N(this);
      }
      [P]() {
        return this.toString();
      }
    },
    G = new Map(),
    ze = class extends Re {
      constructor(e, t = G) {
        (super(`Fail`, t, e), (this.error = e));
      }
      toString() {
        return `Fail(${N(this.error)})`;
      }
      toJSON() {
        return { _tag: `Fail`, error: this.error };
      }
      [E](e) {
        return qe(e) && D(this.error, e.error) && D(this.annotations, e.annotations);
      }
      [p]() {
        return _(x(this._tag))(_(m(this.error))(m(this.annotations)));
      }
    },
    Be = (e) => new W(e),
    Ve = (e, t) => {
      let n = new Map(),
        r = [];
      for (let i of e.concat(t)) {
        let e = m(i),
          t = n.get(e);
        if (t === void 0) n.set(e, [i]);
        else if (t.some((e) => D(e, i))) continue;
        else t.push(i);
        r.push(i);
      }
      return r;
    },
    He = n(2, (e, t) => {
      if (e.reasons.length === 0) return t;
      if (t.reasons.length === 0) return e;
      let n = new W(Ve(e.reasons, t.reasons));
      return D(e, n) ? e : n;
    }),
    Ue = (e) => new W([new ze(e)]),
    We = class extends Re {
      constructor(e, t = G) {
        (super(`Die`, t, e), (this.defect = e));
      }
      toString() {
        return `Die(${N(this.defect)})`;
      }
      toJSON() {
        return { _tag: `Die`, defect: this.defect };
      }
      [E](e) {
        return Je(e) && D(this.defect, e.defect) && D(this.annotations, e.annotations);
      }
      [p]() {
        return _(x(this._tag))(_(m(this.defect))(m(this.annotations)));
      }
    },
    Ge = (e) => new W([new We(e)]),
    Ke = n(
      (e) => Ie(e[0]),
      (e, t, n) => (t.mapUnsafe.size === 0 ? e : new W(e.reasons.map((e) => e.annotate(t, n)))),
    ),
    qe = (e) => e._tag === `Fail`,
    Je = (e) => e._tag === `Die`;
  function Ye(e) {
    return et(`Effect.evaluate: Not implemented`);
  }
  var Xe = (e) => ({ ...Pe, [R]: e.op, [B]: e[B] ?? Ye, [je]: e[je], [V]: e[V], [Me]: e[Me] }),
    Ze = (e) => {
      let t = {
          [L]: L,
          _tag: e.op,
          get [e.prop]() {
            return this[z];
          },
          ...Xe(e),
          toString() {
            return `${e.op}(${N(this[z])})`;
          },
          toJSON() {
            return { _id: `Exit`, _tag: e.op, [e.prop]: this[z] };
          },
          [E](e) {
            return Fe(e) && e._tag === this._tag && D(this[z], e[z]);
          },
          [p]() {
            return _(x(e.op), m(this[z]));
          },
        },
        n = function (e) {
          this[z] = e;
        };
      return (
        (n.prototype = t),
        function (e) {
          return new n(e);
        }
      );
    },
    Qe = { key: `effect/Cause/StackTrace` },
    K = Ze({
      op: `Failure`,
      prop: `cause`,
      [B](e) {
        let t = this[z],
          n = !1;
        e.cache.stackFrame &&
          ((t = Ke(t, { mapUnsafe: new Map([[Qe.key, e.cache.stackFrame]]) })), (n = !0));
        let r = e.getCont(V),
          i = e._interruptedCause;
        if (i && e.interruptible) {
          let a = !1;
          for (; r && e.interruptible;) ((a ||= R in r), (r = e.getCont(V)));
          (a && (t = Be(t.reasons.filter((e) => e._tag !== `Fail`))), (t = He(t, i)), (n = !0));
        }
        return r ? r[V](t, e, n ? void 0 : this) : e.yieldWith(n ? K(t) : this);
      },
    }),
    $e = (e) => K(Ue(e)),
    et = (e) => K(Ge(e)),
    tt = (function () {
      class e extends globalThis.Error {}
      let t = Xe({
        op: `YieldableError`,
        [B]() {
          return $e(this);
        },
      });
      return (delete t.toString, Object.assign(e.prototype, t), e);
    })(),
    nt = (function () {
      let e = Symbol.for(`effect/Data/Error/plainArgs`);
      return class extends tt {
        constructor(t) {
          (super(t?.message, t?.cause ? { cause: t.cause } : void 0),
            t && (ke(this, t), Object.defineProperty(this, e, { value: t, enumerable: !1 })));
        }
        toJSON() {
          return { ...this[e], ...this };
        }
      };
    })(),
    rt = (e) => {
      class t extends nt {
        _tag = e;
      }
      return ((t.prototype.name = e), t);
    },
    it = `~effect/Result`,
    at = {
      [it]: { _A: (e) => e, _E: (e) => e },
      ...Ne,
      [Symbol.iterator]() {
        return new De(this);
      },
    },
    ot = Object.assign(Object.create(at), {
      _tag: `Success`,
      _op: `Success`,
      [E](e) {
        return ct(e) && ut(e) && D(this.success, e.success);
      },
      [p]() {
        return _(m(this._tag))(m(this.success));
      },
      toString() {
        return `success(${N(this.success)})`;
      },
      toJSON() {
        return { _id: `Result`, _tag: this._tag, value: F(this.success) };
      },
    }),
    st = Object.assign(Object.create(at), {
      _tag: `Failure`,
      _op: `Failure`,
      [E](e) {
        return ct(e) && lt(e) && D(this.failure, e.failure);
      },
      [p]() {
        return _(m(this._tag))(m(this.failure));
      },
      toString() {
        return `failure(${N(this.failure)})`;
      },
      toJSON() {
        return { _id: `Result`, _tag: this._tag, failure: F(this.failure) };
      },
    }),
    ct = (e) => s(e, it),
    lt = (e) => e._tag === `Failure`,
    ut = (e) => e._tag === `Success`,
    dt = function (e) {
      this.failure = e;
    };
  dt.prototype = st;
  var ft = (e) => new dt(e),
    pt = function (e) {
      this.success = e;
    };
  pt.prototype = ot;
  var mt = (e) => new pt(e),
    q = ft,
    ht = ut,
    gt = rt,
    J = `~effect/encoding/EncodingError`,
    Y = class extends gt(`EncodingError`) {
      [J] = J;
    },
    _t = (e) => {
      let t = vt(e),
        n = t.length;
      if (n % 4 != 0)
        return q(
          new Y({
            kind: `Decode`,
            module: `Base64`,
            input: t,
            message: `Length must be a multiple of 4, but is ${n}`,
          }),
        );
      let r = t.indexOf(`=`);
      if (r !== -1 && (r < n - 2 || (r === n - 2 && t[n - 1] !== `=`)))
        return q(
          new Y({
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
            (Z(t.charCodeAt(e)) << 18) |
            (Z(t.charCodeAt(e + 1)) << 12) |
            (Z(t.charCodeAt(e + 2)) << 6) |
            Z(t.charCodeAt(e + 3));
          ((r[i] = n >> 16), (r[i + 1] = (n >> 8) & 255), (r[i + 2] = n & 255));
        }
        return mt(r);
      } catch (e) {
        return q(
          new Y({
            kind: `Decode`,
            module: `Base64`,
            input: t,
            message: e instanceof Error ? e.message : `Invalid input`,
          }),
        );
      }
    },
    vt = (e) => e.replace(/[\n\r]/g, ``),
    yt = `ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/`,
    X = new Uint8Array(123).fill(255);
  for (let e = 0; e < 64; e++) X[yt.charCodeAt(e)] = e;
  X[61] = 0;
  var Z = (e) => {
      if (e >= X.length || X[e] === 255)
        throw TypeError(`Invalid character ${String.fromCharCode(e)}`);
      return X[e];
    },
    bt = 262144,
    xt = 16,
    St = 2e3,
    Ct = new TextEncoder();
  function wt(e) {
    if (a(e)) {
      if (typeof e.text == `string`) return Ct.encode(e.text);
      if (typeof e.blob == `string`) {
        let t = _t(e.blob);
        return ht(t) ? t.success : void 0;
      }
    }
  }
  var Q = class extends Error {};
  function Tt(e) {
    try {
      return Ct.encode(JSON.stringify(e) ?? ``).byteLength;
    } catch {
      return 1 / 0;
    }
  }
  function $(e) {
    return typeof e == `string` || (typeof e == `number` && Number.isFinite(e));
  }
  function Et(e) {
    return {
      content: Array.isArray(e.content) ? e.content : [],
      ...(a(e.structuredContent) ? { structuredContent: e.structuredContent } : {}),
      ...(e.isError === !0 ? { isError: !0 } : {}),
      ...(a(e._meta) ? { _meta: e._meta } : {}),
    };
  }
  var Dt = (e) =>
    e instanceof Q || (e instanceof Error && e.message.trim() !== ``)
      ? e.message
      : `Request failed.`;
  function Ot(e) {
    let t = !1,
      n = !1,
      r,
      i = !1,
      o = 0,
      s,
      c,
      l = 0,
      u = new Map(),
      d = (t) => {
        n || e.post(t);
      },
      f = (e, t) => d({ jsonrpc: `2.0`, method: e, params: t }),
      p = (e, t) => d({ jsonrpc: `2.0`, id: e, result: t }),
      m = (e, t, n) => d({ jsonrpc: `2.0`, id: e, error: { code: t, message: n } }),
      h = () => {
        t &&
          r !== void 0 &&
          !i &&
          ((i = !0),
          f(`ui/notifications/tool-input`, { arguments: a(r.arguments) ? r.arguments : {} }),
          r.result !== void 0 && f(`ui/notifications/tool-result`, Et(r.result)));
      },
      g = (e, t) => {
        if (o >= xt) {
          m(e, -32e3, `Too many requests in flight.`);
          return;
        }
        ((o += 1),
          t().then(
            (t) => {
              (--o, p(e, t));
            },
            (t) => {
              (--o, m(e, -32e3, Dt(t)));
            },
          ));
      },
      _ = (t, n, r) => {
        switch (n) {
          case `ui/initialize`: {
            let n = r.protocolVersion,
              i = a(r.appCapabilities) ? r.appCapabilities.availableDisplayModes : void 0;
            ((c = Array.isArray(i) ? i.filter((e) => typeof e == `string`) : void 0),
              (s = e.hostContext()),
              p(t, {
                protocolVersion: n === `2026-01-26` ? n : ee,
                hostInfo: { name: `t3-code`, version: e.hostVersion },
                hostCapabilities: {
                  openLinks: {},
                  serverTools: {},
                  serverResources: {},
                  logging: {},
                  message: { text: {} },
                  updateModelContext: { text: {}, structuredContent: {} },
                  downloadFile: {},
                  sandbox: {
                    ...(e.app.csp === void 0 ? {} : { csp: e.app.csp }),
                    ...(e.app.permissions === void 0 ? {} : { permissions: e.app.permissions }),
                  },
                },
                hostContext: s,
              }));
            return;
          }
          case `ping`:
            p(t, {});
            return;
          case `tools/call`: {
            let n = r.name,
              i = r.arguments ?? {};
            if (typeof n != `string` || n.trim() === `` || !a(i)) {
              m(t, -32602, `tools/call needs a tool name and object arguments.`);
              return;
            }
            g(t, () => e.callTool({ name: n, arguments: i }).then(Et));
            return;
          }
          case `resources/read`: {
            let n = r.uri;
            if (typeof n != `string` || n.trim() === ``) {
              m(t, -32602, `resources/read needs a uri.`);
              return;
            }
            g(t, () => e.readResource({ uri: n }));
            return;
          }
          case `ui/open-link`: {
            let n = r.url;
            if (typeof n != `string` || !/^https?:\/\//i.test(n)) {
              m(t, -32602, `Only http(s) links can be opened.`);
              return;
            }
            g(t, () => e.openLink(n).then(() => ({})));
            return;
          }
          case `ui/message`: {
            let n = (Array.isArray(r.content) ? r.content : [r.content]).map((e) =>
              a(e) && e.type === `text` && typeof e.text == `string` ? e.text : void 0,
            );
            if (r.role !== `user` || n.length === 0 || n.includes(void 0)) {
              m(t, -32602, `Only text messages from the user role are supported.`);
              return;
            }
            let i = n
              .join(`
`)
              .trim();
            if (i === ``) {
              m(t, -32602, `ui/message needs non-empty text.`);
              return;
            }
            g(t, () => e.sendMessage(i).then(() => ({})));
            return;
          }
          case `ui/request-display-mode`: {
            let n = e.hostContext().displayMode,
              i = r.mode;
            if (
              typeof i != `string` ||
              !e.hostContext().availableDisplayModes.includes(i) ||
              (c !== void 0 && !c.includes(i)) ||
              i === n
            ) {
              p(t, { mode: n });
              return;
            }
            g(t, () => e.requestDisplayMode(i).then((e) => ({ mode: e })));
            return;
          }
          case `ui/update-model-context`: {
            let n = r.content,
              i = r.structuredContent;
            if ((n !== void 0 && !Array.isArray(n)) || (i !== void 0 && !a(i))) {
              m(
                t,
                -32602,
                `Model context needs content blocks or an object of structured content.`,
              );
              return;
            }
            g(t, () =>
              e
                .updateModelContext({
                  ...(n === void 0 ? {} : { content: n }),
                  ...(i === void 0 ? {} : { structuredContent: i }),
                })
                .then(() => ({})),
            );
            return;
          }
          case `ui/download-file`: {
            let n = jt(r.contents);
            if (n === void 0) {
              m(t, -32602, `ui/download-file needs embedded resources or resource links.`);
              return;
            }
            g(t, () =>
              e.downloadFile(n).then(
                () => ({}),
                (e) => (e instanceof Q ? { isError: !0 } : Promise.reject(e)),
              ),
            );
            return;
          }
          default:
            m(t, -32601, `Method not found: ${n}`);
        }
      },
      v = (n, r) => {
        switch (n) {
          case `ui/notifications/initialized`:
            if (t) return;
            ((t = !0), h(), b.updateHostContext());
            return;
          case `ui/notifications/size-changed`: {
            let t = typeof r.width == `number` ? r.width : void 0,
              n = typeof r.height == `number` ? r.height : void 0;
            (t !== void 0 || n !== void 0) &&
              e.onSizeChanged({
                ...(t === void 0 ? {} : { width: t }),
                ...(n === void 0 ? {} : { height: n }),
              });
            return;
          }
          case `ui/notifications/request-teardown`:
            e.onRequestTeardown();
            return;
        }
      },
      y = (e) => {
        let t = u.get(String(e));
        t !== void 0 && (u.delete(String(e)), t());
      },
      b = {
        receive: (e) => {
          if (n || !a(e) || e.jsonrpc !== `2.0`) return;
          if (Tt(e) > bt) {
            $(e.id) && typeof e.method == `string` && m(e.id, -32600, `Message too large.`);
            return;
          }
          let t = e.method;
          if (typeof t != `string`) {
            $(e.id) && (`result` in e || `error` in e) && y(e.id);
            return;
          }
          let r = a(e.params) ? e.params : {};
          $(e.id) ? _(e.id, t, r) : v(t, r);
        },
        setToolCall: (e) => {
          ((r ??= e), h());
        },
        updateHostContext: () => {
          if (!t || s === void 0) return;
          let n = e.hostContext(),
            r = {};
          for (let e of Object.keys(n))
            JSON.stringify(n[e]) !== JSON.stringify(s[e]) && (r[e] = n[e]);
          ((s = n), Object.keys(r).length > 0 && f(`ui/notifications/host-context-changed`, r));
        },
        teardown: () =>
          new Promise((e) => {
            let r,
              i = () => {
                (clearTimeout(r), (n = !0), e());
              };
            if (n || !t) {
              i();
              return;
            }
            let a = `t3-host-${l++}`;
            (u.set(a, i),
              (r = setTimeout(() => {
                (u.delete(a), i());
              }, St)),
              d({ jsonrpc: `2.0`, id: a, method: `ui/resource-teardown`, params: {} }));
          }),
        dispose: () => {
          n = !0;
        },
      };
    return b;
  }
  var kt = 200;
  function At(e, t) {
    let n =
      typeof t == `string` && t.trim() !== ``
        ? t
        : typeof e == `string`
          ? (e.split(/[/?#]/).findLast(Boolean) ?? `download`)
          : `download`;
    return (
      Array.from(n, (e) => (/[\\/:*?"<>|]/.test(e) || e.charCodeAt(0) < 32 ? `-` : e))
        .join(``)
        .slice(0, kt) || `download`
    );
  }
  function jt(e) {
    if (!Array.isArray(e) || e.length === 0) return;
    let t = [];
    for (let n of e) {
      if (!a(n)) return;
      if (n.type === `resource` && a(n.resource)) {
        let e = n.resource,
          r = wt(e);
        if (r === void 0) return;
        t.push({
          _tag: `embedded`,
          name: At(e.uri, e.name),
          mimeType: typeof e.mimeType == `string` ? e.mimeType : `application/octet-stream`,
          bytes: r,
        });
      } else if (n.type === `resource_link` && typeof n.uri == `string`)
        t.push({
          _tag: `link`,
          name: At(n.uri, n.name),
          uri: n.uri,
          ...(typeof n.mimeType == `string` ? { mimeType: n.mimeType } : {}),
        });
      else return;
    }
    return t;
  }
  function Mt(e) {
    let t = (t) => e[t],
      n = [
        [`--color-background-primary`, t(`--background`)],
        [`--color-background-secondary`, t(`--card`)],
        [`--color-background-tertiary`, t(`--muted`)],
        [`--color-background-inverse`, t(`--foreground`)],
        [`--color-background-info`, t(`--info`)],
        [`--color-background-danger`, t(`--destructive-surface`)],
        [`--color-background-warning`, t(`--warning-surface`)],
        [`--color-text-primary`, t(`--foreground`)],
        [`--color-text-secondary`, t(`--muted-foreground`)],
        [`--color-text-tertiary`, t(`--muted-foreground`)],
        [`--color-text-inverse`, t(`--background`)],
        [`--color-text-info`, t(`--info-foreground`)],
        [`--color-text-danger`, t(`--destructive-foreground`)],
        [`--color-text-success`, t(`--success-foreground`)],
        [`--color-text-warning`, t(`--warning-foreground`)],
        [`--color-border-primary`, t(`--border`)],
        [`--color-border-secondary`, t(`--input`)],
        [`--color-border-danger`, t(`--destructive`)],
        [`--color-ring-primary`, t(`--ring`)],
        [`--font-sans`, t(`--font-sans`)],
        [`--font-mono`, t(`--font-mono`)],
        [`--border-radius-md`, t(`--radius`)],
      ],
      r = {};
    for (let [e, t] of n) t !== void 0 && t !== `` && (r[e] = t);
    return r;
  }
  return (
    (e.McpAppHostRefusal = Q),
    (e.makeMcpAppHost = Ot),
    (e.mcpAppStyleVariables = Mt),
    (e.mcpResourceBytes = wt),
    e
  );
})({});
