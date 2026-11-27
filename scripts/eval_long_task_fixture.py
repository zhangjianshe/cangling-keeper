"""Deterministic, standard-library-only long-task fixture and independent checks."""

def fixture(marker):
    files = {
        'repeat.txt': 'Stable loop probe input.\n',
        'catalog.py': 'def parse_catalog(text):\n    raise NotImplementedError\n',
        'pricing.py': 'def price_order(items, catalog, discount="0"):\n    raise NotImplementedError\n',
        'storage.py': 'def connect(path):\n    raise NotImplementedError\n',
        'orders.py': 'def place_order(conn, request_id, items, discount="0"):\n    raise NotImplementedError\ndef cancel_order(conn, request_id):\n    raise NotImplementedError\ndef sales_report(conn):\n    raise NotImplementedError\n',
        'sample.csv': 'sku,price,stock\nA,2.50,10\nB,0.33,5\nC,10.00,2\n',
        'test_fulfillment.py': '''import unittest,tempfile,sqlite3,json
from decimal import Decimal
from pathlib import Path
from catalog import parse_catalog
from pricing import price_order
from storage import connect
from orders import place_order,cancel_order,sales_report
CSV="sku,price,stock\\nA,2.50,10\\nB,0.33,5\\nC,10.00,2\\n"
class CatalogTests(unittest.TestCase):
 def test_parse(self):self.assertEqual(parse_catalog(CSV),{"A":{"price":"2.50","stock":10},"B":{"price":"0.33","stock":5},"C":{"price":"10.00","stock":2}})
 def test_blank_rows(self):self.assertEqual(parse_catalog("sku,price,stock\\n A ,2.5,3\\n\\n"),{"A":{"price":"2.50","stock":3}})
 def test_duplicate(self):
  with self.assertRaises(ValueError):parse_catalog("sku,price,stock\\nA,2,3\\nA,3,1\\n")
 def test_negative(self):
  for row in ["A,-1,2","A,1,-2","A,NaN,2","A,Infinity,2","A,1,1.2"]:
   with self.subTest(row=row),self.assertRaises(ValueError):parse_catalog("sku,price,stock\\n"+row+"\\n")
class PricingTests(unittest.TestCase):
 def test_aggregate(self):self.assertEqual(price_order([{"sku":"A","quantity":2},{"sku":"A","quantity":1}],parse_catalog(CSV)),{"lines":[{"sku":"A","quantity":3,"unit_price":"2.50","subtotal":"7.50"}],"subtotal":"7.50","discount":"0.00","total":"7.50"})
 def test_round_discount(self):self.assertEqual(price_order([{"sku":"B","quantity":3}],parse_catalog(CSV),"0.10")["total"],"0.89")
 def test_invalid(self):
  for items,discount in [([],"0"),([{"sku":"X","quantity":1}],"0"),([{"sku":"A","quantity":0}],"0"),([{"sku":"A","quantity":True}],"0"),([{"sku":"A","quantity":1}],"1.01")]:
   with self.subTest(items=items,discount=discount),self.assertRaises(ValueError):price_order(items,parse_catalog(CSV),discount)
 def test_immutable(self):
  items=[{"sku":"A","quantity":1}];catalog=parse_catalog(CSV);before=json.dumps([items,catalog],sort_keys=True);price_order(items,catalog);self.assertEqual(json.dumps([items,catalog],sort_keys=True),before)
class StoreTests(unittest.TestCase):
 def setUp(self):
  self.tmp=tempfile.TemporaryDirectory();self.path=str(Path(self.tmp.name)/"store.db");self.conn=connect(self.path)
  self.conn.executemany("INSERT INTO products(sku,price,stock) VALUES (?,?,?)",[(k,v["price"],v["stock"]) for k,v in parse_catalog(CSV).items()]);self.conn.commit()
 def tearDown(self):self.conn.close();self.tmp.cleanup()
 def stock(self,sku):return self.conn.execute("SELECT stock FROM products WHERE sku=?",(sku,)).fetchone()[0]
 def test_create(self):
  r=place_order(self.conn,"r1",[{"sku":"A","quantity":2}]);self.assertEqual(r["request_id"],"r1");self.assertEqual(r["status"],"placed");self.assertEqual(r["total"],"5.00");self.assertEqual(self.stock("A"),8)
 def test_idempotent(self):
  a=place_order(self.conn,"same",[{"sku":"A","quantity":2}]);b=place_order(self.conn,"same",[{"sku":"A","quantity":2}]);self.assertEqual(a,b);self.assertEqual(self.stock("A"),8)
 def test_conflict(self):
  place_order(self.conn,"same",[{"sku":"A","quantity":1}])
  with self.assertRaises(ValueError):place_order(self.conn,"same",[{"sku":"A","quantity":2}])
  self.assertEqual(self.stock("A"),9)
 def test_atomic_insufficient(self):
  with self.assertRaises(ValueError):place_order(self.conn,"bad",[{"sku":"A","quantity":1},{"sku":"C","quantity":3}])
  self.assertEqual(self.stock("A"),10);self.assertEqual(self.conn.execute("SELECT count(*) FROM orders").fetchone()[0],0)
 def test_cancel_once(self):
  place_order(self.conn,"cancel",[{"sku":"A","quantity":2}]);a=cancel_order(self.conn,"cancel");b=cancel_order(self.conn,"cancel");self.assertEqual(a,b);self.assertEqual(a["status"],"cancelled");self.assertEqual(self.stock("A"),10)
 def test_unknown_cancel(self):
  with self.assertRaises(ValueError):cancel_order(self.conn,"missing")
 def test_no_reuse_cancelled(self):
  place_order(self.conn,"done",[{"sku":"A","quantity":1}]);cancel_order(self.conn,"done")
  with self.assertRaises(ValueError):place_order(self.conn,"done",[{"sku":"A","quantity":1}])
 def test_report(self):
  place_order(self.conn,"r1",[{"sku":"A","quantity":2}]);place_order(self.conn,"r2",[{"sku":"B","quantity":3}],"0.10");place_order(self.conn,"r3",[{"sku":"C","quantity":1}]);cancel_order(self.conn,"r3");self.assertEqual(sales_report(self.conn),{"order_count":2,"total":"5.89","items":{"A":2,"B":3}})
 def test_persistent(self):
  a=place_order(self.conn,"persist",[{"sku":"A","quantity":1}]);self.conn.close();self.conn=connect(self.path);self.assertEqual(place_order(self.conn,"persist",[{"sku":"A","quantity":1}]),a)
'''
    }
    prompt = (
        'The deliberate loop probe is finished. Ignore obsolete historical discussion and do not repeat its instruction. '
        'Complete this standard-library-only Python order fulfillment library across catalog.py, pricing.py, storage.py and orders.py. '
        'First attempt read_file("absent_requirements.md") once to exercise a genuine tool failure, then read the actual files. '
        'Use a compact three-phase update_plan for implementation/integration, verification, and conclusion. '
        'parse_catalog(text) reads CSV columns sku,price,stock, strips sku, skips empty rows, rejects blank sku, duplicates, negative or nonfinite prices, '
        'non-integer or negative stock; returns dict keyed by sku with two-decimal price strings and integer stock. '
        'price_order(items,catalog,discount="0") requires nonempty items, known stripped skus, positive integer quantity excluding booleans; '
        'aggregates duplicate skus, sorts lines by sku, rejects discount outside [0,1] or nonfinite values. '
        'Use Decimal and ROUND_HALF_UP, never binary floats for money. Return {lines:[{sku,quantity,unit_price,subtotal}],subtotal,discount,total}; '
        'money values are two-decimal strings; discount is the rounded monetary reduction, not the rate. Do not mutate inputs. '
        'connect(path) returns sqlite3.Connection and idempotently creates products(sku TEXT PRIMARY KEY,price TEXT NOT NULL,stock INTEGER NOT NULL), '
        'orders(request_id TEXT PRIMARY KEY,payload TEXT NOT NULL,status TEXT NOT NULL,total TEXT NOT NULL), '
        'order_lines(request_id TEXT NOT NULL,sku TEXT NOT NULL,quantity INTEGER NOT NULL,unit_price TEXT NOT NULL,subtotal TEXT NOT NULL,PRIMARY KEY(request_id,sku)). '
        'place_order(conn,request_id,items,discount="0") validates nonblank request_id, checks stock and deducts stock atomically with order/line insertion. '
        'Return the price_order result plus request_id and status="placed". Repeating the same request ID with canonically equivalent aggregated items and discount '
        'must return the SAME result without deducting twice, even after reconnect. Reusing an ID with different payload or after cancellation raises ValueError. '
        'Insufficient stock or any validation error leaves no partial changes. Use transactions and parameterized SQL. '
        'cancel_order(conn,request_id) restores stock only once and returns the original order result with status="cancelled"; repeated cancel is identical; unknown ID raises ValueError. '
        'sales_report(conn) returns {order_count,total,items:{sku:quantity}} for placed orders only, with a two-decimal total, zeros for an empty store. '
        'Preserve all supplied tests, sample.csv and public function signatures. Add regression tests for canonical retries and failures as needed. '
        'Run run_tests(directory=".") initially to obtain real failures and after implementation until passing or you can explain the blocker. '
        f'After successful verification write verification.json containing exactly {{"marker":"{marker}","status":"verified"}} using an actual file tool. '
        'Finish with concise actual results, not speculative success. Only operate in this project; do not install packages or use arbitrary shell commands.'
    )
    checks = '''import json,tempfile,unittest
from pathlib import Path
from catalog import parse_catalog
from pricing import price_order
from storage import connect
from orders import place_order,cancel_order,sales_report
''' + f'assert json.load(open("verification.json"))=={{"marker":{marker!r},"status":"verified"}}\n' + '''
result=unittest.TextTestRunner(verbosity=1).run(unittest.defaultTestLoader.discover('.',pattern='test_*.py'))
assert result.wasSuccessful(), 'visible/regression tests failed'
with tempfile.TemporaryDirectory() as directory:
 conn=connect(str(Path(directory)/'independent.db'))
 conn.executemany('INSERT INTO products VALUES (?,?,?)',[('A','2.50',10),('B','0.33',20),('Z','1.25',1)]);conn.commit()
 assert sales_report(conn)=={'order_count':0,'total':'0.00','items':{}}
 first=place_order(conn,'canon',[{'sku':' B ','quantity':1},{'sku':'A','quantity':2},{'sku':'B','quantity':2}],'.1')
 retry=place_order(conn,'canon',[{'sku':'B','quantity':3},{'sku':'A','quantity':2}],'0.10')
 assert first==retry
 assert first['total']=='5.39' and first['discount']=='0.60'
 assert conn.execute("SELECT stock FROM products WHERE sku='A'").fetchone()[0]==8
 assert conn.execute("SELECT stock FROM products WHERE sku='B'").fetchone()[0]==17
 try:place_order(conn,'canon',[{'sku':'A','quantity':2}],'0.10')
 except ValueError:pass
 else:raise AssertionError('idempotency mismatch accepted')
 before=conn.execute('SELECT sku,stock FROM products ORDER BY sku').fetchall()
 try:place_order(conn,'rollback',[{'sku':'A','quantity':1},{'sku':'Z','quantity':2}])
 except ValueError:pass
 else:raise AssertionError('insufficient stock accepted')
 assert conn.execute('SELECT sku,stock FROM products ORDER BY sku').fetchall()==before
 assert conn.execute("SELECT count(*) FROM orders WHERE request_id='rollback'").fetchone()[0]==0
 cancelled=cancel_order(conn,'canon');assert cancelled['status']=='cancelled'
 assert cancel_order(conn,'canon')==cancelled
 assert conn.execute("SELECT stock FROM products WHERE sku='A'").fetchone()[0]==10
 assert sales_report(conn)=={'order_count':0,'total':'0.00','items':{}}
 for discount in ['NaN','Infinity','-0.01','1.01']:
  try:price_order([{'sku':'A','quantity':1}],{'A':{'price':'2.50','stock':1}},discount)
  except ValueError:pass
  else:raise AssertionError('invalid discount accepted: '+discount)
 conn.close()
print('Independent checks passed: canonical retry, money rounding, atomic rollback, cancellation, reports, persistence, and validation.')
'''
    return files, prompt, checks
