Config = {
  ["outbounds"] = {
    {
      ["chain"] = {
        {
          ["Stdio"] = {
          }
        }
      },
      ["tag"] = "dial1"
    }
  },
  ["inbounds"] = {
    {
      ["chain"] = {
        {
          ["Stdio"] = {
            ["ext"] = {
              ["pre_defined_early_data"] = "abc",
            },
          }
        },
        {
          ["Adder"] = 1
        }
      },
      ["tag"] = "listen1"
    }
  }
}