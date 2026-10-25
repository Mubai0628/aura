use eval_fixture::page;
#[test] fn cursor_is_honored() { assert_eq!(page(&[1,2,3,4],2,2),vec![3,4]); }
#[test] fn boundary_empty() { assert!(page(&[1,2],7,1).is_empty()); }
#[test] fn zero_limit() { assert!(page(&[1],0,0).is_empty()); }
#[test] fn input_unchanged() { let x=vec![1,2,3]; assert_eq!(page(&x,1,8),vec![2,3]); assert_eq!(x,vec![1,2,3]); }
